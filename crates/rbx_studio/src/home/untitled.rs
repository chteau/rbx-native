//! New places that haven't been saved yet. A template opens from a file of
//! its own under the system's temp folder, so nothing lands in the places
//! folder until the user saves: the first save writes `<name>.rbxl` there
//! (or `<name> 2.rbxl`, … when taken) and the session switches to it, as
//! Save to File As does. A place closed without saving takes its file with
//! it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// Untitled places this process has open, removed when it quits.
static OPEN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A file a crashed session left behind is cleared this long after.
const STALE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

fn root() -> PathBuf {
    std::env::temp_dir().join("rbx-native-untitled")
}

/// Whether `path` is an untitled place: one of [`new_path`]'s.
pub(crate) fn is_untitled(path: &Path) -> bool {
    path.starts_with(root())
}

/// This process's own folder of untitled places.
pub(crate) fn dir() -> PathBuf {
    root().join(std::process::id().to_string())
}

/// Where a new place called `name` opens from: in [`dir`], numbered past
/// any other open one of the same name.
pub(crate) fn new_path(name: &str, ext: &str) -> PathBuf {
    free_name(&dir(), name, ext)
}

/// `<dir>/<name>.<ext>`, or the first `<name> <n>.<ext>` not taken.
pub(crate) fn free_name(dir: &Path, name: &str, ext: &str) -> PathBuf {
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{name}.{ext}")),
            n => dir.join(format!("{name} {n}.{ext}")),
        })
        .find(|path| !path.exists())
        .expect("an unbounded range always finds a free name")
}

/// Where the first save of untitled `path` goes in `places`.
pub(crate) fn save_target(path: &Path, places: &Path) -> PathBuf {
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Place");
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("rbxl");
    free_name(places, name, ext)
}

fn lock() -> std::sync::MutexGuard<'static, Vec<PathBuf>> {
    OPEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// An untitled place opened in this process, to clear at quit.
pub(crate) fn opened(path: &Path) {
    let mut open = lock();
    if !open.iter().any(|known| known == path) {
        open.push(path.to_path_buf());
    }
}

/// Keeps `path` past this process (a restart reopens it), handing it to
/// the next one.
pub(crate) fn keep(path: &Path) {
    lock().retain(|known| known != path);
}

/// Removes untitled `path` and its folder once empty.
pub(crate) fn discard(path: &Path) {
    keep(path);
    if is_untitled(path) {
        let _ = std::fs::remove_file(path);
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// At quit: every untitled place still open goes. Then, at most once a
/// launch, what a crashed session left behind long ago.
pub(crate) fn discard_all() {
    let open = std::mem::take(&mut *lock());
    for path in open {
        discard(&path);
    }
}

/// Clears files older than [`STALE`]: a session that crashed never got to
/// [`discard_all`].
pub(crate) fn sweep() {
    sweep_in(&root(), SystemTime::now());
}

fn sweep_in(root: &Path, now: SystemTime) {
    let Ok(dirs) = std::fs::read_dir(root) else {
        return;
    };
    for dir in dirs.flatten().map(|entry| entry.path()) {
        for file in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let old = file
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age > STALE));
            if old {
                let _ = std::fs::remove_file(file.path());
            }
        }
        let _ = std::fs::remove_dir(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_place_is_untitled_and_its_first_save_takes_a_free_name() {
        let path = new_path("Baseplate", "rbxl");
        assert!(is_untitled(&path));
        assert_eq!(path.file_name().unwrap(), "Baseplate.rbxl");
        let places = std::env::temp_dir().join(format!("rbxn-places-{}", std::process::id()));
        std::fs::create_dir_all(&places).unwrap();
        assert!(!is_untitled(&places.join("Baseplate.rbxl")));
        assert_eq!(save_target(&path, &places), places.join("Baseplate.rbxl"));
        std::fs::write(places.join("Baseplate.rbxl"), "").unwrap();
        assert_eq!(save_target(&path, &places), places.join("Baseplate 2.rbxl"));
        std::fs::remove_dir_all(&places).unwrap();
    }

    #[test]
    fn quitting_discards_open_untitled_places_but_not_kept_ones() {
        let dir = root().join(format!("test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (gone, kept) = (dir.join("A.rbxl"), dir.join("B.rbxl"));
        for path in [&gone, &kept] {
            std::fs::write(path, "").unwrap();
            opened(path);
        }
        keep(&kept);
        discard_all();
        assert!(!gone.exists());
        assert!(kept.exists());
        discard(&kept);
        assert!(!dir.exists());
    }

    #[test]
    fn the_sweep_clears_only_stale_files() {
        let root = std::env::temp_dir().join(format!("rbxn-untitled-sweep-{}", std::process::id()));
        let dir = root.join("1234");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Old.rbxl"), "").unwrap();
        sweep_in(&root, SystemTime::now());
        assert!(dir.join("Old.rbxl").exists());
        sweep_in(&root, SystemTime::now() + STALE + Duration::from_secs(60));
        assert!(!dir.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
