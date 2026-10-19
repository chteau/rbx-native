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
//! timestamped name (see [`kept_path`]). A copy a running editor owns is
//! never touched by another: each session holds an OS lock beside its copy,
//! and a second editor on the same place writes its own (see [`claim`]).
//! Unlike
//! Studio's, a recovered copy loses nothing that tied it to its place — a
//! place here is a local file, and the copy is the same kind of file.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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

/// Opens the folder in the file manager, made first if no copy has been
/// written yet so it opens on the folder rather than failing. Settings'
/// Open auto-saves and File › Open Auto Saves both land here.
pub(crate) fn open_folder(cx: &mut gpui_kit::App) {
    if let Some(folder) = folder() {
        let _ = std::fs::create_dir_all(&folder);
        cx.open_with_system(&folder);
    }
}

/// The copy of `place` in `folder`: its own name marked as a recovery copy,
/// in its own format (the extension says which), plus a short hash of its
/// full path so two places both called `Place.rbxl` never share a copy.
///
/// The hash is 64-bit FNV-1a truncated to its low 32 bits, rather than std's
/// `DefaultHasher`, whose output Rust does not promise to keep between
/// releases: a copy written before an update has to be found again after
/// one, to be moved aside when its place next opens. `place` should be
/// canonical, so two spellings of one path share a copy. `slot` is 1 for
/// the first editor on the place; a second one running at once writes slot
/// 2, and so on (see [`claim`]).
pub(crate) fn copy_path(folder: &Path, place: &Path, slot: u32) -> PathBuf {
    let stem = place
        .file_stem()
        .map_or_else(|| "place".into(), |stem| stem.to_string_lossy());
    let extension = place
        .extension()
        .map_or_else(|| "rbxl".into(), |ext| ext.to_string_lossy());
    let hash = fnv1a(place.to_string_lossy().as_bytes()) as u32;
    let slot = if slot > 1 {
        format!(", {slot}")
    } else {
        String::new()
    };
    folder.join(format!("{stem} (recovery {hash:08x}{slot}).{extension}"))
}

/// The lock file beside `copy`. It stays on disk; only the OS lock on it
/// means anything, and the OS drops that when its holder exits or dies.
fn lock_path(copy: &Path) -> PathBuf {
    let mut name = copy.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    copy.with_file_name(name)
}

/// The copy this session writes, and the held lock that makes it this
/// session's: the place's first slot whose lock no running editor holds.
/// Whatever copy already sits in that slot was left by a session that
/// crashed or was killed (a live one would hold the lock), so it is the
/// caller's to move aside. The lock is released when the `File` drops.
pub(crate) fn claim(folder: &Path, place: &Path) -> std::io::Result<(PathBuf, File)> {
    std::fs::create_dir_all(folder)?;
    let mut slot = 1;
    loop {
        let copy = copy_path(folder, place, slot);
        let lock = File::options()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_path(&copy))?;
        match lock.try_lock() {
            Ok(()) => return Ok((copy, lock)),
            Err(TryLockError::WouldBlock) => slot += 1,
            Err(TryLockError::Error(err)) => return Err(err),
        }
    }
}

/// The place's other slots' copies that a crashed or killed session left
/// (no process holds their lock), each with its lock now held so no editor
/// starting meanwhile claims the slot before the caller moves the copy
/// aside. `own` is this session's copy, skipped. Slots are claimed lowest
/// first and lock files stay on disk, so the first slot without one ends
/// the search.
pub(crate) fn orphans(folder: &Path, place: &Path, own: &Path) -> Vec<(PathBuf, File)> {
    let mut found = Vec::new();
    for slot in 1.. {
        let copy = copy_path(folder, place, slot);
        let Ok(lock) = File::options().write(true).open(lock_path(&copy)) else {
            break;
        };
        if copy != own && copy.exists() && lock.try_lock().is_ok() {
            found.push((copy, lock));
        }
    }
    found
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

/// A recovery copy no running editor owns: what Home offers to reopen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Leftover {
    pub(crate) path: PathBuf,
    /// The original place's name (the copy's stem before " (recovery ").
    pub(crate) name: String,
    pub(crate) modified: SystemTime,
}

/// The copies in `folder` that nothing is writing, newest first: those
/// moved aside under a timestamped name, and live-named ones whose lock is
/// free (or was never made). Lock files, save temp files and copies a
/// running editor holds are skipped.
pub(crate) fn leftovers(folder: &Path) -> Vec<Leftover> {
    let mut found: Vec<Leftover> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let file = entry.file_name().to_string_lossy().into_owned();
            let (name, _) = file.split_once(" (recovery ")?;
            if file.ends_with(".lock") || is_temp(&file) || is_held(&path) {
                return None;
            }
            let meta = entry.metadata().ok().filter(|meta| meta.is_file())?;
            Some(Leftover {
                name: name.to_string(),
                modified: meta.modified().ok()?,
                path,
            })
        })
        .collect();
    found.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    found
}

/// Whether a running editor holds `copy`'s lock. The lock is taken only to
/// find out and dropped at once.
fn is_held(copy: &Path) -> bool {
    File::options()
        .write(true)
        .open(lock_path(copy))
        .is_ok_and(|lock| matches!(lock.try_lock(), Err(TryLockError::WouldBlock)))
}

/// Deletes a leftover copy, and its lock file when it is a live-named one
/// that no editor holds. A copy an editor has since claimed is left alone.
pub(crate) fn discard(copy: &Path) -> std::io::Result<()> {
    if is_held(copy) {
        return Ok(());
    }
    std::fs::remove_file(copy)?;
    let _ = std::fs::remove_file(lock_path(copy));
    Ok(())
}

/// A temp file `save::save` left behind (`<name>.tmp-<pid>`), when the
/// process writing it died before the rename.
pub(crate) fn is_temp(name: &str) -> bool {
    name.rsplit_once(".tmp-")
        .is_some_and(|(_, pid)| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
}

pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
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
        let copy = copy_path(Path::new("/r"), Path::new("/games/Obby.rbxlx"), 1);
        let name = copy.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("Obby (recovery "), "{name}");
        assert!(name.ends_with(").rbxlx"), "{name}");
        assert_eq!(copy.parent(), Some(Path::new("/r")));
    }

    #[test]
    fn two_places_with_one_name_get_two_copies_and_one_place_always_the_same() {
        let folder = Path::new("/r");
        let a = copy_path(folder, Path::new("/one/Place.rbxl"), 1);
        let b = copy_path(folder, Path::new("/two/Place.rbxl"), 1);
        assert_ne!(a, b);
        assert_eq!(a, copy_path(folder, Path::new("/one/Place.rbxl"), 1));
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
        let copy = copy_path(Path::new("/r"), Path::new("/games/Obby.rbxl"), 1);
        let kept = kept_path(&copy, "2026-10-02 14-03-11");
        assert_ne!(kept, copy);
        let name = kept.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("Obby (recovery "), "{name}");
        assert!(name.ends_with(") 2026-10-02 14-03-11.rbxl"), "{name}");
    }

    #[test]
    fn a_second_editors_copy_and_lock_never_share_the_firsts_names() {
        let place = Path::new("/games/Obby.rbxl");
        let first = copy_path(Path::new("/r"), place, 1);
        let second = copy_path(Path::new("/r"), place, 2);
        assert_ne!(first, second);
        let name = second.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("Obby (recovery "), "{name}");
        assert!(name.ends_with(", 2).rbxl"), "{name}");
        assert_eq!(
            lock_path(&first),
            Path::new(&format!("{}.lock", first.display()))
        );
        assert!(!is_temp(&lock_path(&first).to_string_lossy()));
    }

    /// Each `File` holds its own lock (flock on Unix, LockFileEx on
    /// Windows), so two claims in one process behave like two editors.
    #[test]
    fn a_held_lock_sends_the_next_editor_to_its_own_copy_and_a_dropped_one_frees_it() {
        let folder =
            std::env::temp_dir().join(format!("rbx-recovery-claim-{}", std::process::id()));
        let place = Path::new("/games/Obby.rbxl");
        let (first, lock) = claim(&folder, place).unwrap();
        assert_eq!(first, copy_path(&folder, place, 1));
        let (second, _second_lock) = claim(&folder, place).unwrap();
        assert_eq!(second, copy_path(&folder, place, 2));
        // The first editor exits (or dies): its slot is free again.
        drop(lock);
        assert_eq!(claim(&folder, place).unwrap().0, first);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_crashed_editors_higher_slot_copy_is_found_but_a_running_ones_never_is() {
        let folder =
            std::env::temp_dir().join(format!("rbx-recovery-orphans-{}", std::process::id()));
        let place = Path::new("/games/Obby.rbxl");
        let (first, _first_lock) = claim(&folder, place).unwrap();
        let (second, second_lock) = claim(&folder, place).unwrap();
        std::fs::write(&second, b"work").unwrap();
        // The second editor is still running: its copy is left alone.
        assert!(orphans(&folder, place, &first).is_empty());
        // It crashes: its copy is the first editor's to move aside.
        drop(second_lock);
        let found: Vec<_> = orphans(&folder, place, &first)
            .into_iter()
            .map(|(copy, _)| copy)
            .collect();
        assert_eq!(found, [second]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn leftovers_list_unheld_copies_newest_first_and_discard_removes_them() {
        let folder =
            std::env::temp_dir().join(format!("rbx-recovery-leftovers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let obby = Path::new("/games/Obby.rbxl");
        let (live, _held) = claim(&folder, obby).unwrap();
        std::fs::write(&live, b"running").unwrap();
        let (crashed, crashed_lock) = claim(&folder, obby).unwrap();
        std::fs::write(&crashed, b"crashed").unwrap();
        drop(crashed_lock);
        let kept = kept_path(
            &copy_path(&folder, Path::new("/g/Tower.rbxlx"), 1),
            "2026-10-02 14-03-11",
        );
        std::fs::write(&kept, b"kept").unwrap();
        std::fs::write(folder.join("Obby (recovery 0a1b2c3d).rbxl.tmp-42"), b"t").unwrap();
        std::fs::write(folder.join("notes.txt"), b"n").unwrap();
        let time = SystemTime::now() + Duration::from_secs(60);
        File::options()
            .write(true)
            .open(&kept)
            .unwrap()
            .set_modified(time)
            .unwrap();

        let found = leftovers(&folder);
        let paths: Vec<_> = found.iter().map(|left| left.path.clone()).collect();
        assert_eq!(paths, [kept.clone(), crashed.clone()]);
        assert_eq!(found[0].name, "Tower");
        assert_eq!(found[1].name, "Obby");

        discard(&crashed).unwrap();
        assert!(!crashed.exists() && !lock_path(&crashed).exists());
        discard(&live).unwrap();
        assert!(live.exists(), "a held copy survives");
        assert_eq!(leftovers(&folder).len(), 1);
        let _ = std::fs::remove_dir_all(&folder);
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
