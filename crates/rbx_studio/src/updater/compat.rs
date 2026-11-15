//! Keeping true what other installs expect after an update: the Linux
//! 0.1.0 executable name, and the Windows setup's Add/Remove Programs entry.

#[cfg(windows)]
use semver::Version;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

/// Linux 0.1.0 shipped the editor as `rbxstudio`, and its updater only
/// looks for that name, so releases keep a package with it. Run from that
/// name, the editor renames itself to [`EXE_NAME`] and leaves `rbxstudio`
/// as a symlink to it, so desktop entries and scripts made for the old name
/// keep working. The new path when it renamed.
///
/// [`EXE_NAME`]: super::release::EXE_NAME
#[cfg(unix)]
pub(super) fn migrate_legacy_name(exe: &Path) -> Option<PathBuf> {
    use super::release::{EXE_NAME, LEGACY_EXE_NAME};
    if exe.file_name()? != LEGACY_EXE_NAME || exe.is_symlink() {
        return None;
    }
    let new = exe.with_file_name(EXE_NAME);
    if new.exists() || new.is_symlink() {
        return None;
    }
    std::fs::rename(exe, &new).ok()?;
    if std::os::unix::fs::symlink(EXE_NAME, exe).is_err() {
        let _ = std::fs::rename(&new, exe);
        return None;
    }
    Some(new)
}

/// The Windows setup's uninstall key: `AppId` in
/// `packaging/windows/RbxNative.iss`, plus Inno Setup's `_is1`.
#[cfg(windows)]
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{39CCCF6D-DAD2-464F-A1AB-97143C3F610D}_is1";

/// Add/Remove Programs shows the version the setup installed; an in-place
/// update moves it along.
#[cfg(windows)]
pub(super) fn record_version(dir: &Path, version: &Version) {
    use std::os::windows::process::CommandExt;
    // The setup's uninstaller sits in the folder it installed to; a
    // portable copy has none.
    if !dir.join("unins000.exe").is_file() {
        return;
    }
    let reg = |args: &[&str]| {
        std::process::Command::new("reg")
            .args(args)
            .creation_flags(crate::luau_lsp::CREATE_NO_WINDOW)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    // `reg add` would create the key; only touch the one the setup made.
    if reg(&["query", UNINSTALL_KEY, "/v", "DisplayVersion"]) {
        let version = version.to_string();
        let set = ["add", UNINSTALL_KEY, "/v", "DisplayVersion", "/t", "REG_SZ"];
        reg(&[&set[..], &["/d", &version, "/f"]].concat());
    }
}
