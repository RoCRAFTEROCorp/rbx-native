//! Auto-Recovery's shell side: noticing changes, the timer, and writing the
//! copy off the UI thread (see `crate::recovery` for what and where).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use gpui_kit::*;

use super::Shell;
use crate::recovery;
use crate::save;
use crate::script_editor::source;

/// How often the timer looks. Well under the shortest interval, so a copy
/// lands within a few seconds of falling due.
const CHECK_EVERY: Duration = Duration::from_secs(15);

/// A temp file this old in the recovery folder belongs to a process that
/// died mid-write; a live write takes seconds.
const STALE_TEMP: Duration = Duration::from_secs(60 * 60);

pub(super) struct Recovery {
    enabled: bool,
    minutes: u32,
    /// The place's canonical path: the copy's name hashes it, so a relative
    /// and an absolute launch of one place share a copy.
    place: PathBuf,
    /// No copies this session: the place is itself inside the recovery
    /// folder, or an earlier session's copy could not be moved aside.
    off: bool,
    /// The place changed since the last copy (or since it was saved).
    changed: bool,
    /// When the last copy was taken; the place opening counts as one, so
    /// the first copy waits a full interval.
    last: Instant,
    /// A copy is being written, so the next check does not start another.
    writing: bool,
    /// This session wrote the copy now in the folder, so a save may delete
    /// it. Nothing else is ever deleted.
    wrote: bool,
    /// Bumped by every save, so a copy that finishes writing after one
    /// knows it is stale and deletes itself.
    saves: u64,
}

impl Recovery {
    pub(super) fn new(enabled: bool, minutes: u32, place: &Path) -> Self {
        Recovery {
            enabled,
            minutes: recovery::clamp_minutes(u64::from(minutes)),
            place: std::fs::canonicalize(place).unwrap_or_else(|_| place.to_path_buf()),
            off: false,
            changed: false,
            last: Instant::now(),
            writing: false,
            wrote: false,
            saves: 0,
        }
    }

    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn minutes(&self) -> u32 {
        self.minutes
    }

    /// Called from `Shell::push_history_snapshot`, which every recorded
    /// edit (script runs and `Source` writes included) passes through, and
    /// from `Shell::reflect_changes`, which undo, redo and a drag's later
    /// steps do.
    pub(super) fn changed(&mut self) {
        self.changed = true;
    }
}

impl Shell {
    pub(super) fn watch_recovery(&mut self, cx: &mut Context<Self>) {
        self.open_recovery();
        cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(CHECK_EVERY).await;
            if shell
                .update(cx, |shell, cx| shell.write_recovery(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    /// Once, as the place opens (the editor holds one place for its whole
    /// life): sweeps temp files dead writers left, refuses to copy a place
    /// that is itself in the recovery folder, and moves an earlier
    /// session's copy of this place aside so this session never overwrites
    /// or deletes it.
    fn open_recovery(&mut self) {
        let Some(folder) = recovery::folder() else {
            return;
        };
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age > STALE_TEMP);
            if old && recovery::is_temp(&entry.file_name().to_string_lossy()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }

        let state = &mut self.recovery;
        if std::fs::canonicalize(&folder).is_ok_and(|folder| state.place.starts_with(folder)) {
            state.off = true;
            self.output.push_warning(
                "This place is in the Auto-Recovery folder, so no recovery copies are written \
                 of it, and Ctrl+S saves it there. To keep working on it, copy it out of that \
                 folder and open it from there.",
            );
            return;
        }
        let copy = recovery::copy_path(&folder, &state.place);
        let Ok(meta) = std::fs::metadata(&copy) else {
            return;
        };
        let written = meta.modified().unwrap_or_else(|_| SystemTime::now());
        let stamp = chrono::DateTime::<chrono::Local>::from(written)
            .format("%Y-%m-%d %H-%M-%S")
            .to_string();
        let mut kept = recovery::kept_path(&copy, &stamp);
        let mut n = 1;
        while kept.exists() {
            n += 1;
            kept = recovery::kept_path(&copy, &format!("{stamp} ({n})"));
        }
        match std::fs::rename(&copy, &kept) {
            Ok(()) => self.output.push_warning(&format!(
                "Auto-Recovery found a copy of this place from an earlier session and kept \
                 it as {} (Studio Settings › Files & recovery › Open auto-saves).",
                kept.display()
            )),
            Err(err) => {
                state.off = true;
                self.output.push_warning(&format!(
                    "Auto-Recovery found a copy of this place from an earlier session at {} \
                     but could not move it aside ({err}), so it writes no copies this \
                     session, leaving that one as it is.",
                    copy.display()
                ));
            }
        }
    }

    /// Writes a copy if one is due. The DOM is cloned here, on the UI thread
    /// (a large place's clone is a short hitch), and serialized and written
    /// on a background one. `save::save` writes through a temp file, so a
    /// copy is never left half-written.
    fn write_recovery(&mut self, cx: &mut Context<Self>) {
        let state = &self.recovery;
        if state.off
            || state.writing
            || !recovery::due(
                state.enabled,
                state.changed,
                state.last.elapsed(),
                state.minutes,
            )
        {
            return;
        }
        let Some(folder) = recovery::folder() else {
            return;
        };
        let path = recovery::copy_path(&folder, &self.recovery.place);
        let mut dom = self.dom.clone();
        // Typing still on its debounce goes into the copy, but not through
        // `flush_script_edits`, which would cut an undo step mid-word.
        for (reference, open) in &self.scripts.open {
            if open.pending {
                source::write(&mut dom, *reference, &open.state.read(cx).value());
            }
        }
        let format = self.format;
        let saves = self.recovery.saves;
        self.recovery.changed = false;
        self.recovery.last = Instant::now();
        self.recovery.writing = true;
        self.recovery.wrote = true;

        let written = path.clone();
        let write = cx.background_executor().spawn(async move {
            std::fs::create_dir_all(&folder).map_err(|err| err.to_string())?;
            save::save(&dom, format, &path)
        });
        cx.spawn(async move |shell, cx| {
            let result = write.await;
            let _ = shell.update(cx, |shell, cx| {
                shell.recovery.writing = false;
                if shell.recovery.saves != saves {
                    // A save landed while this was written: nothing is left
                    // to recover, and this copy would outlive it.
                    let _ = std::fs::remove_file(&written);
                } else if let Err(message) = result {
                    // Still unsaved, so the next check tries again.
                    shell.recovery.changed = true;
                    shell
                        .output
                        .push_warning(&format!("Auto-Recovery could not write a copy: {message}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// After a successful Ctrl+S: nothing is left to recover, so the copy
    /// this session wrote goes. One still being written deletes itself when
    /// it lands (see `write_recovery`).
    pub(super) fn saved(&mut self) {
        let state = &mut self.recovery;
        state.changed = false;
        state.last = Instant::now();
        state.saves += 1;
        if std::mem::take(&mut state.wrote) {
            if let Some(folder) = recovery::folder() {
                let _ = std::fs::remove_file(recovery::copy_path(&folder, &state.place));
            }
        }
    }

    pub(super) fn set_auto_recovery(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.recovery.enabled != enabled {
            self.recovery.enabled = enabled;
            self.save_settings();
            cx.notify();
        }
    }

    pub(super) fn set_recovery_minutes(&mut self, minutes: u32, cx: &mut Context<Self>) {
        let minutes = recovery::clamp_minutes(u64::from(minutes));
        if self.recovery.minutes != minutes {
            self.recovery.minutes = minutes;
            self.save_settings();
            cx.notify();
        }
    }
}
