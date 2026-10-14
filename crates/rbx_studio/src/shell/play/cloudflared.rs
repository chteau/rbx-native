//! The `cloudflared` release built into this binary (see `build.rs`), the
//! tunnel Play's game server reaches the editor through. Unpacked once into
//! the cache folder the first time Play needs it, into a folder named after
//! its version, and written under a temporary name then renamed so two
//! editors starting at once never run a half-written file.

use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

const ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/cloudflared.zip"));
pub(crate) const VERSION: &str = env!("CLOUDFLARED_VERSION");
/// cloudflared's Apache-2.0 licence, kept next to the binary it covers.
const LICENSE: &str = include_str!("../../../third_party/cloudflared-LICENSE.txt");
/// The entry `build.rs` stores the binary under.
const ENTRY: &str = "cloudflared";

const EXECUTABLE: &str = if cfg!(windows) {
    "cloudflared.exe"
} else {
    "cloudflared"
};

/// The program Play starts: `$RBX_CLOUDFLARED` when set, else the bundled
/// one (unpacked on first use), else `cloudflared` on `PATH` for a target
/// with nothing bundled. Blocking on first use (~40 MB written).
pub(crate) fn program() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("RBX_CLOUDFLARED") {
        return Ok(PathBuf::from(path));
    }
    if ZIP.is_empty() {
        return Ok(PathBuf::from(EXECUTABLE));
    }
    let dir = rbx_assets::cache_root()
        .unwrap_or_else(std::env::temp_dir)
        .join("cloudflared")
        .join(VERSION);
    unpack(ZIP, &dir)
}

fn unpack(zip: &[u8], dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join(EXECUTABLE);
    let mut archive = zip::ZipArchive::new(io::Cursor::new(zip)).map_err(io::Error::other)?;
    let mut entry = archive.by_name(ENTRY).map_err(io::Error::other)?;
    if fs::metadata(&path).is_ok_and(|meta| meta.len() == entry.size()) {
        return Ok(path);
    }

    fs::create_dir_all(dir)?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    let partial = dir.join(format!("{EXECUTABLE}.{}.partial", std::process::id()));
    fs::write(&partial, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&partial, &path)?;
    fs::write(dir.join("LICENSE.txt"), LICENSE)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    /// The bundled binary unpacks and runs; skipped on a target with none.
    #[test]
    fn the_bundled_cloudflared_unpacks_and_reports_its_version() {
        if super::ZIP.is_empty() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rbx-cloudflared-test-{}", std::process::id()));
        let path = super::unpack(super::ZIP, &dir).expect("unpacks");
        let output = Command::new(&path).arg("--version").output().expect("runs");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains(super::VERSION), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
