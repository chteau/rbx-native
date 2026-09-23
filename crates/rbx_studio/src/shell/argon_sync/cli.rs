//! Where the `argon` CLI is on this machine, by looking for the file —
//! never by running it. Auto Connect asks this before trying anything,
//! so a place opened on a machine without Argon stays quietly
//! disconnected instead of failing red on every launch.
//!
//! The lookup covers `PATH` and then the places an install lands that a
//! GUI app's `PATH` may not have picked up: Argon's own installer and its
//! VS Code extension both put the binary in `~/.argon/bin`
//! (`argon-rbx/argon@main:src/installer.rs`, `get_argon_dir()/bin`;
//! `argon-rbx/argon-vscode@main:src/installer.ts`), the Rokit / Aftman /
//! Foreman toolchain managers keep theirs under their own `~/.<tool>/bin`,
//! `cargo install` uses `$CARGO_HOME/bin`, and on macOS Homebrew's
//! `/opt/homebrew/bin` and `/usr/local/bin` are missing from an app
//! launched from Finder.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The CLI on this machine, if any: [`find_argon_cli`] over the live
/// environment. Looked up fresh every time, so an install made
/// mid-session counts at the next place open.
pub(super) fn locate() -> Option<PathBuf> {
    let path = std::env::var_os("PATH");
    let home = std::env::home_dir();
    let cargo_home = std::env::var_os("CARGO_HOME").map(PathBuf::from);
    find_argon_cli(path.as_deref(), home.as_deref(), cargo_home.as_deref())
}

/// The first `argon` binary in: every absolute `PATH` entry,
/// `<home>/.argon/bin`, the toolchain managers' bins, Cargo's bin, and
/// (macOS) Homebrew's. A Unix match must be a regular file with an
/// executable bit; a Windows match is `argon.exe`, or on `PATH` any
/// `PATHEXT` extension. A relative or empty `PATH` entry is skipped: it
/// would resolve against the app's working directory, and a file that
/// happens to sit there is not an install.
pub(super) fn find_argon_cli(
    path: Option<&OsStr>,
    home: Option<&Path>,
    cargo_home: Option<&Path>,
) -> Option<PathBuf> {
    let on_path = path
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| candidates(&dir, true))
        .find(|candidate| is_executable(candidate));
    if on_path.is_some() {
        return on_path;
    }

    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".argon").join("bin"));
        for tool in [".rokit", ".aftman", ".foreman"] {
            dirs.push(home.join(tool).join("bin"));
        }
    }
    match cargo_home {
        Some(cargo_home) => dirs.push(cargo_home.join("bin")),
        None => {
            if let Some(home) = home {
                dirs.push(home.join(".cargo").join("bin"));
            }
        }
    }
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    dirs.iter()
        .flat_map(|dir| candidates(dir, false))
        .find(|candidate| is_executable(candidate))
}

/// The file names one directory could hold the CLI under. `PATHEXT`
/// only applies to `PATH` entries, as the shell would apply it.
fn candidates(dir: &Path, from_path: bool) -> Vec<PathBuf> {
    if cfg!(windows) {
        let mut names = vec![dir.join("argon.exe")];
        if from_path {
            if let Some(exts) = std::env::var_os("PATHEXT") {
                for ext in exts.to_string_lossy().split(';') {
                    let ext = ext.trim();
                    if !ext.is_empty() && !ext.eq_ignore_ascii_case(".exe") {
                        names.push(dir.join(format!("argon{ext}")));
                    }
                }
            }
        }
        names
    } else {
        vec![dir.join("argon")]
    }
}

fn is_executable(candidate: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(candidate) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temp dir, removed on drop.
    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "rbx-native-argon-cli-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a temp sandbox");
            Sandbox(dir)
        }

        fn dir(&self, relative: &str) -> PathBuf {
            let dir = self.0.join(relative);
            std::fs::create_dir_all(&dir).expect("a sandbox dir");
            dir
        }

        /// Writes an `argon` file into `relative`, executable unless
        /// `executable` is false, and returns its path.
        fn binary(&self, relative: &str, executable: bool) -> PathBuf {
            let name = if cfg!(windows) { "argon.exe" } else { "argon" };
            let file = self.dir(relative).join(name);
            std::fs::write(&file, b"#!/bin/sh\n").expect("a fake binary");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = if executable { 0o755 } else { 0o644 };
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode))
                    .expect("permissions");
            }
            let _ = executable;
            file
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn found_on_path() {
        let sandbox = Sandbox::new("path");
        let binary = sandbox.binary("somewhere/bin", true);
        let path = std::env::join_paths([sandbox.dir("empty"), sandbox.dir("somewhere/bin")])
            .expect("a PATH");
        assert_eq!(
            find_argon_cli(Some(&path), Some(&sandbox.dir("home")), None),
            Some(binary)
        );
    }

    #[test]
    fn found_only_in_the_argon_bin_with_an_empty_path() {
        let sandbox = Sandbox::new("argon-bin");
        let home = sandbox.dir("home");
        let binary = sandbox.binary("home/.argon/bin", true);
        assert_eq!(
            find_argon_cli(Some(OsStr::new("")), Some(&home), None),
            Some(binary)
        );
        assert_eq!(
            find_argon_cli(None, Some(&home), None),
            Some(sandbox.binary("home/.argon/bin", true))
        );
    }

    #[test]
    fn found_in_the_rokit_bin() {
        let sandbox = Sandbox::new("rokit");
        let home = sandbox.dir("home");
        let binary = sandbox.binary("home/.rokit/bin", true);
        assert_eq!(find_argon_cli(None, Some(&home), None), Some(binary));
    }

    #[test]
    fn cargo_home_overrides_the_default_cargo_bin() {
        let sandbox = Sandbox::new("cargo");
        let home = sandbox.dir("home");
        let in_default = sandbox.binary("home/.cargo/bin", true);
        let custom = sandbox.dir("custom-cargo");
        let in_custom = sandbox.binary("custom-cargo/bin", true);
        assert_eq!(
            find_argon_cli(None, Some(&home), Some(&custom)),
            Some(in_custom)
        );
        assert_eq!(find_argon_cli(None, Some(&home), None), Some(in_default));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_an_executable_bit_is_ignored() {
        let sandbox = Sandbox::new("noexec");
        let home = sandbox.dir("home");
        sandbox.binary("home/.argon/bin", false);
        assert_eq!(find_argon_cli(None, Some(&home), None), None);
    }

    #[test]
    fn a_relative_or_empty_path_entry_never_matches_the_working_directory() {
        let sandbox = Sandbox::new("relative");
        let home = sandbox.dir("home");
        sandbox.binary("relative/bin", true);
        // Neither an empty entry nor a relative one may reach into wherever
        // the process happens to run from; the sandbox has the file at
        // exactly the relative path named here, if that were resolved
        // against it.
        assert_eq!(
            find_argon_cli(Some(OsStr::new("")), Some(&home), None),
            None
        );
        assert_eq!(
            find_argon_cli(Some(OsStr::new("relative/bin")), Some(&home), None),
            None
        );
    }

    #[test]
    fn nothing_present_returns_none() {
        let sandbox = Sandbox::new("none");
        let home = sandbox.dir("home");
        let path = std::env::join_paths([sandbox.dir("bin")]).expect("a PATH");
        assert_eq!(
            find_argon_cli(Some(&path), Some(&home), Some(&sandbox.dir("cargo"))),
            None
        );
    }
}
