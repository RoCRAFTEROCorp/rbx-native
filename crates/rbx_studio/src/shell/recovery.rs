//! Auto-Recovery's shell side: noticing changes, the timer, and writing the
//! copy off the UI thread (see `crate::recovery` for what and where).

use std::time::{Duration, Instant};

use gpui_kit::*;

use super::Shell;
use crate::recovery;
use crate::save;

/// How often the timer looks. Well under the shortest interval, so a copy
/// lands within a few seconds of falling due.
const CHECK_EVERY: Duration = Duration::from_secs(15);

pub(super) struct Recovery {
    enabled: bool,
    minutes: u32,
    /// The place changed since the last copy (or since it was saved).
    changed: bool,
    /// When the last copy was taken; the place opening counts as one, so
    /// the first copy waits a full interval.
    last: Instant,
    /// A copy is being written, so the next check does not start another.
    writing: bool,
}

impl Recovery {
    pub(super) fn new(enabled: bool, minutes: u32) -> Self {
        Recovery {
            enabled,
            minutes: recovery::clamp_minutes(u64::from(minutes)),
            changed: false,
            last: Instant::now(),
            writing: false,
        }
    }

    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn minutes(&self) -> u32 {
        self.minutes
    }

    /// Called from `Shell::reflect_changes`, which every edit, script run
    /// and undo passes through.
    pub(super) fn changed(&mut self) {
        self.changed = true;
    }
}

impl Shell {
    pub(super) fn watch_recovery(&self, cx: &mut Context<Self>) {
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

    /// Writes a copy if one is due. The DOM is cloned here, on the UI thread,
    /// and serialized and written on a background one, so a large place
    /// never stalls a frame. `save::save` writes through a temp file, so a
    /// copy is never left half-written.
    fn write_recovery(&mut self, cx: &mut Context<Self>) {
        let state = &self.recovery;
        if state.writing
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
        self.flush_script_edits(cx);
        let path = recovery::copy_path(&folder, &self.path);
        let dom = self.dom.clone();
        let format = self.format;
        self.recovery.changed = false;
        self.recovery.last = Instant::now();
        self.recovery.writing = true;

        let write = cx.background_executor().spawn(async move {
            std::fs::create_dir_all(&folder).map_err(|err| err.to_string())?;
            save::save(&dom, format, &path)
        });
        cx.spawn(async move |shell, cx| {
            let result = write.await;
            let _ = shell.update(cx, |shell, cx| {
                shell.recovery.writing = false;
                if let Err(message) = result {
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

    /// After a successful Ctrl+S: nothing is left to recover, so this
    /// place's copy goes. A copy still being written when the save lands is
    /// left; it is a moment older than the save, and the next save or copy
    /// replaces it.
    pub(super) fn saved(&mut self) {
        self.recovery.changed = false;
        self.recovery.last = Instant::now();
        if let Some(folder) = recovery::folder() {
            let _ = std::fs::remove_file(recovery::copy_path(&folder, &self.path));
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
