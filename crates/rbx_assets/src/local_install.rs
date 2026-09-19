//! Fallback texture source: a Roblox or Studio install on this machine, read
//! straight off disk.
//!
//! The Windows installers unpack the `rbxasset://` tree the Studio content
//! packages carry into `%LOCALAPPDATA%\Roblox\Versions\<version>\content\`
//! (and some textures into a second root, see [`CONTENT_ROOTS`]), so a file
//! the CDN cannot serve (offline, or missing from its packages) is often
//! sitting in a directory next to Studio.
//! [`crate::native::NativeContent`] tries this only after its own CDN attempt
//! has already failed — `setup.rbxcdn.com` stays the primary source — and,
//! like the Sober fallback, never writes to or installs anything: it only
//! reads files the user's own install already put there.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// Every version folder the installers create starts with this.
const VERSION_PREFIX: &str = "version-";

/// Where inside one version folder a `rbxasset://` path can live, in the
/// order they are tried.
///
/// `content` holds most of the tree (`sky/sun.jpg`, `fonts/…`, `textures/…`).
/// The default skybox panels the viewer asks for as `sky/sky512_*.tex` are
/// not there: a real install keeps them under `PlatformContent\pc\textures\`,
/// so the last root is the same folder seen one level down, which the
/// Studio content package spells with the same `sky\` prefix.
const CONTENT_ROOTS: [&[&str]; 3] = [
    &["content"],
    &["PlatformContent", "pc"],
    &["PlatformContent", "pc", "textures"],
];

/// The `Versions` directory of a local Roblox install.
pub(crate) struct LocalInstall {
    versions_dir: PathBuf,
}

impl LocalInstall {
    /// Builds a [`LocalInstall`] against `%LOCALAPPDATA%\Roblox\Versions`.
    /// `None` when `LOCALAPPDATA` is unset, which is the case on every
    /// platform Roblox has no Windows installer for.
    pub(crate) fn new() -> Option<Self> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")?;
        Some(Self::with_versions_dir(versions_dir_under(Path::new(
            &local_app_data,
        ))))
    }

    /// Builds a [`LocalInstall`] against an arbitrary directory; tests use
    /// this to stand a temp dir in for the real install.
    pub(crate) fn with_versions_dir(versions_dir: PathBuf) -> Self {
        Self { versions_dir }
    }

    /// Reads `relative_path` (e.g. `"textures/face.png"`, the same string a
    /// `rbxasset://` reference strips to) from the newest installed version
    /// that holds it. `None` on any failure — no install, an invalid path,
    /// the file absent from every version — because this only ever backs a
    /// best-effort fallback whose caller keeps the CDN's own error.
    pub(crate) fn read(&self, relative_path: &str) -> Option<Vec<u8>> {
        let relative = safe_relative_path(relative_path)?;
        // Older versions stay on disk after an update and may still hold a
        // file a newer one dropped, so a miss in the newest is not the end.
        self.version_dirs().into_iter().find_map(|version| {
            CONTENT_ROOTS.iter().find_map(|root| {
                let base = root
                    .iter()
                    .fold(version.clone(), |dir, part| dir.join(part));
                fs::read(base.join(&relative)).ok()
            })
        })
    }

    /// Every `version-*` directory, newest first.
    fn version_dirs(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.versions_dir) else {
            return Vec::new();
        };
        let versions = entries
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(VERSION_PREFIX)
                    && entry.file_type().is_ok_and(|kind| kind.is_dir())
            })
            .map(|entry| {
                let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
                (entry.path(), modified)
            })
            .collect();
        newest_first(versions)
    }
}

/// `<local app data>\Roblox\Versions`, where the installers put every
/// version they have ever installed.
fn versions_dir_under(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Roblox").join("Versions")
}

/// The directory names carry a random hash, not an ordering, so the
/// modification time is the only real signal of which install is current.
/// A directory whose time could not be read sorts last.
fn newest_first(mut versions: Vec<(PathBuf, Option<SystemTime>)>) -> Vec<PathBuf> {
    versions.sort_by(|(_, a), (_, b)| b.cmp(a));
    versions.into_iter().map(|(path, _)| path).collect()
}

/// `path` as a relative filesystem path, or `None` when it could reach
/// outside the content root it is about to be joined onto.
///
/// The string comes from a place file, which is not trusted: unlike the zip
/// packages the other fallbacks read, this one hands it to the filesystem, so
/// `..`, a drive or root prefix, or an NTFS stream separator would each read
/// an arbitrary file the user never meant to expose.
fn safe_relative_path(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.contains(['\\', ':']) {
        return None;
    }

    let relative = Path::new(path);
    let only_names = relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    // `components` silently drops empty segments and `.`, so `a//b` and
    // `a/./b` would pass the check above; refuse them so a path is only
    // ever read as the exact string the place file wrote.
    let literal = path.split('/').all(|part| !part.is_empty() && part != ".");
    (only_names && literal).then(|| relative.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rbx_assets_local_install_test_{}_{n}",
            std::process::id()
        ))
    }

    /// Writes `content/<relative>` under `version` in a fake `Versions` dir.
    fn install_file(versions: &Path, version: &str, relative: &str, bytes: &[u8]) {
        let file = versions.join(version).join("content").join(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }

    #[test]
    fn versions_live_under_local_app_data_roblox_versions() {
        assert_eq!(
            versions_dir_under(Path::new("appdata")),
            Path::new("appdata").join("Roblox").join("Versions")
        );
    }

    #[test]
    fn reads_a_file_from_a_versions_content_directory() {
        let versions = temp_dir();
        install_file(&versions, "version-aaaa", "textures/face.png", b"pngdata");

        let install = LocalInstall::with_versions_dir(versions);
        assert_eq!(
            install.read("textures/face.png").as_deref(),
            Some(&b"pngdata"[..])
        );
    }

    /// Writes `<root>/<relative>` under `version`, where `root` is any path
    /// below the version folder rather than only `content`.
    fn install_file_at(versions: &Path, version: &str, root: &str, relative: &str, bytes: &[u8]) {
        let file = versions.join(version).join(root).join(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }

    // The layout observed on a real Studio install: the default skybox panels
    // are not in `content` at all.
    #[test]
    fn default_sky_panels_come_from_platform_content() {
        let versions = temp_dir();
        install_file_at(
            &versions,
            "version-aaaa",
            "PlatformContent/pc/textures",
            "sky/sky512_up.tex",
            b"panel",
        );

        let install = LocalInstall::with_versions_dir(versions);
        assert_eq!(
            install.read("sky/sky512_up.tex").as_deref(),
            Some(&b"panel"[..])
        );
    }

    #[test]
    fn a_path_is_also_looked_up_directly_under_platform_content() {
        let versions = temp_dir();
        install_file_at(
            &versions,
            "version-aaaa",
            "PlatformContent/pc",
            "textures/studs.dds",
            b"studs",
        );

        let install = LocalInstall::with_versions_dir(versions);
        assert_eq!(
            install.read("textures/studs.dds").as_deref(),
            Some(&b"studs"[..])
        );
    }

    #[test]
    fn content_is_preferred_over_platform_content_within_a_version() {
        let versions = temp_dir();
        install_file_at(
            &versions,
            "version-aaaa",
            "content",
            "sky/x.dds",
            b"content",
        );
        install_file_at(
            &versions,
            "version-aaaa",
            "PlatformContent/pc/textures",
            "sky/x.dds",
            b"platform",
        );

        let install = LocalInstall::with_versions_dir(versions);
        assert_eq!(install.read("sky/x.dds").as_deref(), Some(&b"content"[..]));
    }

    #[test]
    fn reads_nested_paths() {
        let versions = temp_dir();
        install_file(&versions, "version-aaaa", "sky/nested/deep.dds", b"dds");

        let install = LocalInstall::with_versions_dir(versions);
        assert_eq!(
            install.read("sky/nested/deep.dds").as_deref(),
            Some(&b"dds"[..])
        );
    }

    // Roblox leaves the previous version on disk after an update, and a file
    // the newest one dropped is still worth having over a missing texture.
    #[test]
    fn falls_back_to_a_version_that_still_holds_the_file() {
        let versions = temp_dir();
        install_file(&versions, "version-aaaa", "textures/only_here.png", b"old");
        install_file(&versions, "version-bbbb", "textures/other.png", b"new");

        let install = LocalInstall::with_versions_dir(versions);
        assert_eq!(
            install.read("textures/only_here.png").as_deref(),
            Some(&b"old"[..])
        );
    }

    #[test]
    fn a_file_in_no_version_is_none() {
        let versions = temp_dir();
        install_file(&versions, "version-aaaa", "textures/face.png", b"x");

        let install = LocalInstall::with_versions_dir(versions);
        assert_eq!(install.read("textures/missing.png"), None);
    }

    #[test]
    fn a_missing_versions_directory_is_none_not_a_panic() {
        let install = LocalInstall::with_versions_dir(temp_dir());
        assert_eq!(install.read("textures/face.png"), None);
    }

    #[test]
    fn only_version_directories_are_searched() {
        let versions = temp_dir();
        // A file, not a directory, carrying the right prefix; and a directory
        // with the wrong one. Neither is an install.
        fs::create_dir_all(&versions).unwrap();
        fs::write(versions.join("version-file"), b"not a dir").unwrap();
        install_file(&versions, "RobloxStudioInstaller", "textures/x.png", b"no");

        let install = LocalInstall::with_versions_dir(versions);
        assert!(install.version_dirs().is_empty());
        assert_eq!(install.read("textures/x.png"), None);
    }

    #[test]
    fn newer_installs_sort_before_older_ones_and_unknown_times_last() {
        let base = SystemTime::UNIX_EPOCH;
        let ordered = newest_first(vec![
            (PathBuf::from("unknown"), None),
            (PathBuf::from("old"), Some(base + Duration::from_secs(10))),
            (PathBuf::from("new"), Some(base + Duration::from_secs(99))),
        ]);

        assert_eq!(
            ordered,
            [
                PathBuf::from("new"),
                PathBuf::from("old"),
                PathBuf::from("unknown")
            ]
        );
    }

    #[test]
    fn ordinary_relative_paths_are_accepted() {
        assert_eq!(
            safe_relative_path("textures/face.png"),
            Some(PathBuf::from("textures/face.png"))
        );
        assert!(safe_relative_path("sky/sky512_up.tex").is_some());
    }

    #[test]
    fn paths_that_could_leave_the_content_directory_are_refused() {
        for path in [
            "",
            "..",
            "../secret.txt",
            "textures/../../secret.txt",
            "textures/./face.png",
            "textures//face.png",
            "textures/",
            "/etc/passwd",
            "\\windows\\system.ini",
            "textures\\..\\..\\secret.txt",
            "C:/Windows/system.ini",
            "C:\\Windows\\system.ini",
            "textures/face.png:stream",
        ] {
            assert_eq!(safe_relative_path(path), None, "{path:?} must be refused");
        }
    }

    /// Reads the machine's real Roblox install, so it can only run where one
    /// exists; everything above stands a temp directory in for it.
    #[test]
    #[ignore = "reads the real %LOCALAPPDATA%\\Roblox\\Versions install"]
    fn serves_real_files_from_the_machines_own_install() {
        let install = LocalInstall::new().expect("LOCALAPPDATA is not set");

        let sun = install.read("sky/sun.jpg").expect("sky/sun.jpg not found");
        assert_eq!(sun[..3], [0xFF, 0xD8, 0xFF], "not a JPEG");
        let panel = install
            .read("sky/sky512_up.tex")
            .expect("sky/sky512_up.tex not found");
        assert_eq!(&panel[..4], b"DDS ", "not a DDS texture");
    }

    // The end-to-end version of the table above: a real file sits just
    // outside `content`, and no spelling of a path may reach it.
    #[test]
    fn a_file_outside_the_content_directory_is_never_read() {
        let versions = temp_dir();
        install_file(&versions, "version-aaaa", "textures/face.png", b"ok");
        fs::write(versions.join("version-aaaa").join("secret.txt"), b"secret").unwrap();
        fs::write(versions.join("secret.txt"), b"secret").unwrap();

        let install = LocalInstall::with_versions_dir(versions);
        assert_eq!(install.read("../secret.txt"), None);
        assert_eq!(install.read("../../secret.txt"), None);
        assert_eq!(install.read("textures/../../secret.txt"), None);
    }
}
