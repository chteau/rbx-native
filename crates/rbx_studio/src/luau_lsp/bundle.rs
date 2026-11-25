//! The `luau-lsp` release built into this binary (see `build.rs`), unpacked
//! once into the cache folder the first time it is needed.
//!
//! Unpacked into a folder named after its version, so a newer editor never
//! runs an older server left behind, and written under a temporary name
//! then renamed, so two editors starting at once never run a half-written
//! file.

use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

const ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/luau-lsp.zip"));
pub(crate) const VERSION: &str = env!("LUAU_LSP_VERSION");
/// luau-lsp's MIT licence and that of the Luau it is built from, kept next
/// to the binary it covers.
const NOTICE: &str = include_str!("../../third_party/luau-lsp-NOTICE.txt");

const EXECUTABLE: &str = if cfg!(windows) {
    "luau-lsp.exe"
} else {
    "luau-lsp"
};

/// The bundled server, unpacked; `None` on a target with no release to
/// bundle.
pub(super) fn binary(cache: &Path) -> io::Result<Option<PathBuf>> {
    if ZIP.is_empty() {
        return Ok(None);
    }
    unpack(ZIP, &cache.join("bin").join(VERSION)).map(Some)
}

/// `run`, which starts the unpacked server, tried again while the system
/// says the file is busy: a process another thread forks while a write
/// handle to the binary is open (ours as we unpack, or anything's) holds a
/// copy of that handle until it execs.
pub(super) fn retry_busy<T>(mut run: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    for _ in 0..50 {
        match run() {
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            done => return done,
        }
    }
    run()
}

fn unpack(zip: &[u8], dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join(EXECUTABLE);
    let mut archive = zip::ZipArchive::new(io::Cursor::new(zip)).map_err(io::Error::other)?;
    let mut entry = archive.by_name(EXECUTABLE).map_err(io::Error::other)?;
    if fs::metadata(&path).is_ok_and(|meta| meta.len() == entry.size()) {
        return Ok(path);
    }

    fs::create_dir_all(dir)?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    // Threads of one process can unpack at once (tests do), so the name has
    // to be unique per call, not just per process.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let partial = dir.join(format!("{EXECUTABLE}.{}-{n}.partial", std::process::id()));
    fs::write(&partial, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o755))?;
    }
    if let Err(error) = fs::rename(&partial, &path) {
        // Another thread got there first (and on Windows may be running it).
        let _ = fs::remove_file(&partial);
        if !fs::metadata(&path).is_ok_and(|meta| meta.len() == entry.size()) {
            return Err(error);
        }
    }
    fs::write(dir.join("NOTICE.txt"), NOTICE)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::{binary, retry_busy, ZIP};

    /// The embedded release runs and is the version `build.rs` pinned.
    #[test]
    fn the_bundled_server_unpacks_and_runs() {
        if ZIP.is_empty() {
            return;
        }
        let cache = std::env::temp_dir().join(format!("rbx-luau-bundle-{}", std::process::id()));
        let path = binary(&cache).unwrap().unwrap();
        // A second call finds it already there.
        assert_eq!(binary(&cache).unwrap().unwrap(), path);
        assert!(path.with_file_name("NOTICE.txt").exists());

        let output = retry_busy(|| Command::new(&path).arg("--version").output()).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            super::VERSION
        );
        let _ = std::fs::remove_dir_all(cache);
    }
}
