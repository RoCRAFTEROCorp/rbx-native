//! Auto-Recovery: a copy of the open place, written in the background every
//! few minutes while it has changes Ctrl+S has not written, so a crash or a
//! killed process loses minutes of work rather than everything since the
//! last save. Real Studio's own Auto-Recovery (Studio Settings › Studio ›
//! Auto-Recovery) is the model; the shell side lives in `shell::recovery`.
//!
//! One copy per place, overwritten each time, in `<config>/recovery`. A
//! manual save deletes it: what it held is now in the place itself. A copy
//! an earlier session left behind (it crashed, or was killed) is never
//! overwritten or deleted: opening its place moves it aside under a
//! timestamped name (see [`kept_path`]). Unlike
//! Studio's, a recovered copy loses nothing that tied it to its place — a
//! place here is a local file, and the copy is the same kind of file.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The Interval row's stops, in minutes. Studio's own range runs from a
/// minute or two up to ten.
pub(crate) const INTERVAL_MINUTES: (u32, u32) = (1, 10);

/// A first run's interval: Studio's default is five minutes or so; four sits
/// on a stop and reads as "a few".
pub(crate) const INTERVAL_DEFAULT: u32 = 4;

/// Out-of-range minutes from a hand-edited file land on the nearest stop.
pub(crate) fn clamp_minutes(minutes: u64) -> u32 {
    let (low, high) = INTERVAL_MINUTES;
    minutes.clamp(u64::from(low), u64::from(high)) as u32
}

/// Where the copies go.
pub(crate) fn folder() -> Option<PathBuf> {
    crate::settings::default_config_dir().map(|dir| dir.join("recovery"))
}

/// The copy of `place` in `folder`: its own name marked as a recovery copy,
/// in its own format (the extension says which), plus a short hash of its
/// full path so two places both called `Place.rbxl` never share a copy.
///
/// The hash is 64-bit FNV-1a truncated to its low 32 bits, rather than std's
/// `DefaultHasher`, whose output Rust does not promise to keep between
/// releases: a copy written before an update has to be found again after
/// one, to be moved aside when its place next opens. `place` should be
/// canonical, so two spellings of one path share a copy.
pub(crate) fn copy_path(folder: &Path, place: &Path) -> PathBuf {
    let stem = place
        .file_stem()
        .map_or_else(|| "place".into(), |stem| stem.to_string_lossy());
    let extension = place
        .extension()
        .map_or_else(|| "rbxl".into(), |ext| ext.to_string_lossy());
    let hash = fnv1a(place.to_string_lossy().as_bytes()) as u32;
    folder.join(format!("{stem} (recovery {hash:08x}).{extension}"))
}

/// Where a copy an earlier session left behind is moved to: its own name
/// plus `stamp` (when that copy was written), so it is never overwritten by
/// this session's copies nor deleted by its saves.
pub(crate) fn kept_path(copy: &Path, stamp: &str) -> PathBuf {
    let stem = copy.file_stem().unwrap_or_default().to_string_lossy();
    let name = match copy.extension() {
        Some(ext) => format!("{stem} {stamp}.{}", ext.to_string_lossy()),
        None => format!("{stem} {stamp}"),
    };
    copy.with_file_name(name)
}

/// A temp file `save::save` left behind (`<name>.tmp-<pid>`), when the
/// process writing it died before the rename.
pub(crate) fn is_temp(name: &str) -> bool {
    name.rsplit_once(".tmp-")
        .is_some_and(|(_, pid)| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Whether a copy should be written now: switched on, something to save,
/// and a full interval since the last copy (or since the place opened).
pub(crate) fn due(enabled: bool, changed: bool, since_last: Duration, minutes: u32) -> bool {
    enabled && changed && since_last >= Duration::from_secs(u64::from(minutes) * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_keeps_the_places_name_and_format() {
        let copy = copy_path(Path::new("/r"), Path::new("/games/Obby.rbxlx"));
        let name = copy.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("Obby (recovery "), "{name}");
        assert!(name.ends_with(").rbxlx"), "{name}");
        assert_eq!(copy.parent(), Some(Path::new("/r")));
    }

    #[test]
    fn two_places_with_one_name_get_two_copies_and_one_place_always_the_same() {
        let folder = Path::new("/r");
        let a = copy_path(folder, Path::new("/one/Place.rbxl"));
        let b = copy_path(folder, Path::new("/two/Place.rbxl"));
        assert_ne!(a, b);
        assert_eq!(a, copy_path(folder, Path::new("/one/Place.rbxl")));
    }

    /// Pinned, so a change to the hash (which would orphan every copy
    /// written before it) shows up as a failing test rather than as stale
    /// files nobody deletes.
    #[test]
    fn the_name_hash_is_stable() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn a_kept_copy_never_shares_the_live_copys_name() {
        let copy = copy_path(Path::new("/r"), Path::new("/games/Obby.rbxl"));
        let kept = kept_path(&copy, "2026-10-02 14-03-11");
        assert_ne!(kept, copy);
        let name = kept.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("Obby (recovery "), "{name}");
        assert!(name.ends_with(") 2026-10-02 14-03-11.rbxl"), "{name}");
    }

    #[test]
    fn only_save_temp_files_are_swept() {
        assert!(is_temp("Obby (recovery 0a1b2c3d).rbxl.tmp-4242"));
        assert!(!is_temp("Obby (recovery 0a1b2c3d).rbxl"));
        assert!(!is_temp("notes.tmp-old"));
    }

    #[test]
    fn a_copy_is_due_only_when_on_changed_and_an_interval_has_passed() {
        let four = Duration::from_secs(4 * 60);
        assert!(due(true, true, four, 4));
        assert!(!due(true, true, four - Duration::from_secs(1), 4));
        assert!(!due(false, true, four, 4));
        assert!(!due(true, false, four, 4));
    }

    #[test]
    fn minutes_are_clamped_to_the_rows_stops() {
        assert_eq!(clamp_minutes(0), 1);
        assert_eq!(clamp_minutes(7), 7);
        assert_eq!(clamp_minutes(600), 10);
    }
}
