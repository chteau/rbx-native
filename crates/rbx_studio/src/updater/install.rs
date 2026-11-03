//! Putting a verified package in place of the running copy, for the next
//! launch to run.
//!
//! - Linux, and a bare macOS binary: rename the new executable over the
//!   running one. Same folder, so the rename is atomic; the running process
//!   keeps the old file's inode until it exits.
//! - Windows can't replace a running .exe, but can rename it: the running one
//!   becomes `rbxstudio.old.exe`, the new one takes its name, and the next
//!   launch deletes the old one ([`Location::clear_leftovers`]).
//! - A macOS `.app`: the whole bundle is swapped the same way (`X.app` →
//!   `X.app.old`, new bundle → `X.app`), so its signature seal and
//!   Info.plist always match the binary inside.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use super::release::EXE_NAME;

/// Where the files are unpacked, next to the install so every rename stays
/// on one filesystem: `<STAGING>-<pid>`, one per process, so a second copy
/// of the editor starting up can't pull an in-flight update out from under
/// the first.
const STAGING: &str = ".rbxstudio-update";

/// A staging folder untouched this long belongs to a process that died
/// mid-update; a live download can't stall that long (see `updater::agent`).
const STALE_STAGING: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// `FLATPAK_ID` is set, or the sandbox's `/.flatpak-info` exists.
pub(super) fn in_flatpak(id: Option<OsString>, info: &Path) -> bool {
    id.is_some_and(|id| !id.is_empty()) || info.exists()
}

/// Whether this user can create (and so rename) files in `dir`.
pub(super) fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".rbxstudio-write-check-{}", std::process::id()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => fs::remove_file(&probe).is_ok(),
        Err(_) => false,
    }
}

/// What gets replaced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Location {
    Binary(PathBuf),
    /// A macOS `.app` the executable runs from.
    Bundle(PathBuf),
}

impl Location {
    pub(super) fn current() -> Result<Location, String> {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        Ok(Location::of(&running_path(exe)))
    }

    pub(super) fn of(exe: &Path) -> Location {
        match bundle_of(exe) {
            Some(bundle) => Location::Bundle(bundle),
            None => Location::Binary(exe.to_owned()),
        }
    }

    /// The folder whose entries change.
    pub(super) fn dir(&self) -> &Path {
        let (Location::Binary(path) | Location::Bundle(path)) = self;
        path.parent().unwrap_or(Path::new("."))
    }

    /// Replaces this install with the package unpacked at `unpacked`.
    pub(super) fn replace_from(&self, unpacked: &Path) -> Result<(), String> {
        let result = match self {
            Location::Bundle(bundle) => {
                let new = find(unpacked, &|path| {
                    path.is_dir() && path.extension().is_some_and(|ext| ext == "app")
                })
                .ok_or("the package has no .app bundle")?;
                swap_aside(&new, bundle, &old_bundle(bundle)).map(|()| {
                    // Already out of the way; the running process doesn't
                    // need the files to be there.
                    let _ = fs::remove_dir_all(old_bundle(bundle));
                })
            }
            Location::Binary(exe) => {
                let new = find(unpacked, &|path| {
                    path.is_file() && path.file_name().is_some_and(|name| name == EXE_NAME)
                })
                .ok_or_else(|| format!("the package has no {EXE_NAME}"))?;
                if cfg!(windows) {
                    swap_aside(&new, exe, &old_exe(exe))
                } else {
                    swap_over(&new, exe)
                }
            }
        };
        result.map_err(|error| format!("could not replace the install: {error}"))
    }

    /// What an earlier swap left: Windows' `rbxstudio.old.exe`, a bundle's
    /// `.app.old`, an interrupted download's staging folder (only a stale
    /// one, another running copy may own a fresh one).
    pub(super) fn clear_leftovers(&self) {
        match self {
            Location::Binary(exe) => {
                let _ = fs::remove_file(old_exe(exe));
            }
            Location::Bundle(bundle) => {
                let _ = fs::remove_dir_all(old_bundle(bundle));
            }
        }
        clear_stale_staging(self.dir(), STALE_STAGING);
    }
}

pub(super) fn clear_stale_staging(dir: &Path, older_than: std::time::Duration) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let ours = entry.file_name().to_string_lossy().starts_with(STAGING);
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age >= older_than);
        if ours && stale {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Where the install is, once this process's own file has been swapped out:
/// Linux names it `/path/rbxstudio (deleted)`, Windows follows the rename to
/// `rbxstudio.old.exe`.
pub(super) fn running_path(exe: PathBuf) -> PathBuf {
    let Some(path) = exe.to_str() else {
        return exe;
    };
    if let Some(path) = path.strip_suffix(" (deleted)") {
        return PathBuf::from(path);
    }
    match path.strip_suffix(".old.exe") {
        Some(stem) => PathBuf::from(format!("{stem}.exe")),
        None => exe,
    }
}

/// `X.app` for `X.app/Contents/MacOS/<exe>`.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then(|| bundle.to_owned())
}

/// `rbxstudio.old.exe` beside `rbxstudio.exe`.
pub(super) fn old_exe(exe: &Path) -> PathBuf {
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned());
    exe.with_file_name(format!(
        "{}.old.exe",
        stem.as_deref().unwrap_or("rbxstudio")
    ))
}

fn old_bundle(bundle: &Path) -> PathBuf {
    let mut name = bundle.as_os_str().to_owned();
    name.push(".old");
    PathBuf::from(name)
}

/// Unix: the new file takes the running one's name in one rename.
pub(super) fn swap_over(new: &Path, exe: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(new, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(new, exe)
}

/// Moves `current` to `old`, then `new` to `current`; puts `current` back if
/// the second rename fails, so a failure never leaves nothing to launch.
pub(super) fn swap_aside(new: &Path, current: &Path, old: &Path) -> std::io::Result<()> {
    if old.is_dir() {
        fs::remove_dir_all(old)?;
    } else if old.exists() {
        fs::remove_file(old)?;
    }
    fs::rename(current, old)?;
    if let Err(error) = fs::rename(new, current) {
        let _ = fs::rename(old, current);
        return Err(error);
    }
    Ok(())
}

/// The first path under `dir` (breadth first, a few levels) that `wanted`
/// accepts. A bundle is just a folder here, so a bare macOS binary can be
/// fed the one inside a package's `.app`.
fn find(dir: &Path, wanted: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut level = vec![dir.to_owned()];
    for _ in 0..4 {
        let mut next = Vec::new();
        for dir in level {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            entries.sort();
            for path in entries {
                if wanted(&path) {
                    return Some(path);
                }
                if path.is_dir() {
                    next.push(path);
                }
            }
        }
        level = next;
    }
    None
}

/// Unpacks a `.tar.gz` or `.zip` into `into`. Both readers refuse entries
/// that would land outside it.
pub(super) fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let failed = |error: &dyn std::fmt::Display| format!("could not unpack the update: {error}");
    fs::create_dir_all(into).map_err(|e| failed(&e))?;
    let file = fs::File::open(archive).map_err(|e| failed(&e))?;
    let name = archive.to_string_lossy();
    if name.ends_with(".zip") {
        zip::ZipArchive::new(file)
            .and_then(|mut zip| zip.extract(into))
            .map_err(|e| failed(&e))
    } else if name.ends_with(".tar.gz") {
        tar::Archive::new(flate2::read::GzDecoder::new(file))
            .unpack(into)
            .map_err(|e| failed(&e))
    } else {
        Err(failed(&"not a .zip or .tar.gz"))
    }
}

/// The staging folder, removed when dropped.
pub(super) struct Staging(PathBuf);

impl Staging {
    pub(super) fn new(dir: &Path) -> Result<Staging, String> {
        let path = dir.join(format!("{STAGING}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(Staging(path))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
