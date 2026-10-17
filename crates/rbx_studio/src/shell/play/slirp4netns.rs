//! The `slirp4netns` release built into this binary (see `build.rs`): the
//! user-mode network that gives Play's shaped network namespace internet
//! (see `rbx_play::netsim`). GPL-2.0, shipped unmodified as a separate
//! program with its licence and source pointer beside it. Unpacked once into
//! the cache folder, like `cloudflared`.

use std::io;
use std::path::PathBuf;

const ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/slirp4netns.zip"));
const VERSION: &str = env!("SLIRP4NETNS_VERSION");
/// The GPL text plus the pointer to the corresponding source, kept next to
/// the binary it covers.
const LICENSE: &str = include_str!("../../../third_party/slirp4netns-LICENSE.txt");
/// The entry `build.rs` stores the binary under.
const ENTRY: &str = "slirp4netns";
const EXECUTABLE: &str = "slirp4netns";

/// The program Play starts: `$RBX_SLIRP4NETNS` when set, else the bundled
/// one (unpacked on first use), else `slirp4netns` on `PATH` for a build
/// with nothing bundled.
pub(in crate::shell) fn program() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("RBX_SLIRP4NETNS") {
        return Ok(PathBuf::from(path));
    }
    if ZIP.is_empty() {
        return Ok(PathBuf::from(EXECUTABLE));
    }
    let dir = rbx_assets::cache_root()
        .unwrap_or_else(std::env::temp_dir)
        .join("slirp4netns")
        .join(VERSION);
    super::cloudflared::unpack(ZIP, &dir, ENTRY, EXECUTABLE, LICENSE)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    /// The bundled binary unpacks and runs; skipped when none is bundled.
    #[test]
    fn the_bundled_slirp4netns_unpacks_and_reports_its_version() {
        if super::ZIP.is_empty() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rbx-slirp4netns-test-{}", std::process::id()));
        let path = crate::shell::play::cloudflared::unpack(
            super::ZIP,
            &dir,
            super::ENTRY,
            super::EXECUTABLE,
            super::LICENSE,
        )
        .expect("unpacks");
        let output = Command::new(&path).arg("--version").output().expect("runs");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains(super::VERSION), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
