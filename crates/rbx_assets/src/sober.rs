//! Fallback texture source: Sober (`org.vinegarhq.Sober`), the unofficial
//! Roblox client for Linux distributed as a Flatpak. Once it has been
//! launched at least once, it holds a real Roblox Android client APK (a
//! zip) bundling most of the same `rbxasset://textures/...` files Studio's
//! own content packages carry — a useful alternative when
//! `setup.rbxcdn.com` lacks a file or is unreachable.
//! [`crate::native::NativeContent`] tries this only after its own CDN
//! attempt has already failed.
//!
//! Nothing here touches the network or installs software on its own:
//! [`Sober::install`] runs `flatpak install`, but only when a caller
//! explicitly invokes it — see its doc comment.

mod apk;
mod error;

use std::path::PathBuf;
use std::process::Command;

pub use error::SoberError;

const FLATPAK_APP_ID: &str = "org.vinegarhq.Sober";

/// Locates and reads from a local Sober (Flatpak) installation's data.
pub struct Sober {
    /// `~/.var/app/org.vinegarhq.Sober/data/sober`, Flatpak's well-known
    /// per-app data directory. Injectable so tests never touch `$HOME`.
    data_dir: PathBuf,
}

impl Sober {
    /// Builds a [`Sober`] against the real Flatpak data directory under
    /// `$HOME`. Returns `None` if `$HOME` is unset.
    pub fn new() -> Option<Self> {
        let home = std::env::var_os("HOME")?;
        Some(Self::with_data_dir(
            PathBuf::from(home)
                .join(".var/app")
                .join(FLATPAK_APP_ID)
                .join("data/sober"),
        ))
    }

    /// Builds a [`Sober`] against an arbitrary data directory; tests use
    /// this to stand a temp dir in for `$HOME` without touching the real
    /// Flatpak install.
    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    /// Whether `org.vinegarhq.Sober` is installed via Flatpak. Any failure
    /// to run `flatpak` (binary missing, non-zero exit, unparsable output)
    /// is treated as "not installed" — this only ever gates a best-effort
    /// fallback, so it must never panic or propagate an error.
    pub fn is_installed() -> bool {
        Self::is_installed_via("flatpak")
    }

    fn is_installed_via(flatpak_bin: &str) -> bool {
        Command::new(flatpak_bin)
            .args(["list", "--app", "--columns=application"])
            .output()
            .map(|out| {
                out.status.success()
                    && String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .any(|line| line.trim() == FLATPAK_APP_ID)
            })
            .unwrap_or(false)
    }

    /// Whether Sober has been launched at least once, i.e. it has already
    /// downloaded the Roblox Android client APK this module reads from.
    pub fn has_run_once(&self) -> bool {
        self.find_base_apk().is_some()
    }

    /// Human-readable instruction for the *user* to run themselves. This is
    /// the default path every caller should show; nothing in this crate
    /// executes it automatically.
    pub fn install_prompt() -> &'static str {
        "Sober is not installed. To enable it as a texture fallback, run:\n\
         flatpak install flathub org.vinegarhq.Sober"
    }

    /// Runs `flatpak install -y flathub org.vinegarhq.Sober`, installing
    /// real software on the user's system.
    ///
    /// **No code in this crate calls this automatically.** It exists only
    /// for a caller (an editor UI action, a CLI flag the user explicitly
    /// passed) that has already obtained the user's explicit consent —
    /// mirroring this project's standing rule against invasive system
    /// actions without confirmation. Show [`Sober::install_prompt`] by
    /// default instead.
    pub fn install(&self) -> Result<(), SoberError> {
        let status = Command::new("flatpak")
            .args(["install", "-y", "flathub", FLATPAK_APP_ID])
            .status()
            .map_err(|e| SoberError::Io(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(SoberError::Io(format!(
                "flatpak install exited with status {status}"
            )))
        }
    }

    /// Extracts `relative_path` (e.g. `"textures/face.png"`, the same
    /// string a `rbxasset://` reference strips to) from Sober's downloaded
    /// Roblox APK.
    pub fn extract_texture(&self, relative_path: &str) -> Result<Vec<u8>, SoberError> {
        let apk_path = match self.find_base_apk() {
            Some(p) => p,
            None => return Err(Self::absence_error(Self::is_installed())),
        };
        let bytes = std::fs::read(&apk_path).map_err(|e| SoberError::Io(e.to_string()))?;
        apk::extract_from_apk(&bytes, relative_path)
    }

    /// Classifies "no APK found" as either "never launched" (installed) or
    /// "not installed", split out so the branch is testable without
    /// invoking the real `flatpak` binary from a test.
    fn absence_error(installed: bool) -> SoberError {
        if installed {
            SoberError::NeverRun
        } else {
            SoberError::NotInstalled
        }
    }

    /// Finds `packages/*/com.roblox.client/base.apk`; the arch segment
    /// (`x86_64`, ...) is discovered rather than assumed.
    fn find_base_apk(&self) -> Option<PathBuf> {
        let packages_dir = self.data_dir.join("packages");
        std::fs::read_dir(&packages_dir)
            .ok()?
            .flatten()
            .find_map(|entry| {
                let apk = entry.path().join("com.roblox.client").join("base.apk");
                apk.is_file().then_some(apk)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("rbx_assets_sober_test_{}_{n}", std::process::id()))
    }

    #[test]
    fn has_run_once_is_false_without_packages_dir() {
        let sober = Sober::with_data_dir(temp_dir());
        assert!(!sober.has_run_once());
    }

    #[test]
    fn has_run_once_finds_apk_under_any_arch_dir() {
        let data_dir = temp_dir();
        let apk_dir = data_dir.join("packages/aarch64/com.roblox.client");
        std::fs::create_dir_all(&apk_dir).unwrap();
        std::fs::write(apk_dir.join("base.apk"), b"pkzip-stub").unwrap();

        let sober = Sober::with_data_dir(data_dir);
        assert!(sober.has_run_once());
    }

    #[test]
    fn has_run_once_ignores_arch_dirs_without_the_apk() {
        let data_dir = temp_dir();
        std::fs::create_dir_all(data_dir.join("packages/x86_64/com.roblox.client")).unwrap();

        let sober = Sober::with_data_dir(data_dir);
        assert!(!sober.has_run_once());
    }

    /// Never touches the real Flatpak install: points `flatpak_bin` at a
    /// binary that cannot exist, so `Command` fails to spawn.
    #[test]
    fn is_installed_via_missing_binary_is_false_not_a_panic() {
        assert!(!Sober::is_installed_via(
            "definitely-not-a-real-binary-rbx-sober-test"
        ));
    }

    /// Exercises the output-parsing branch against a fake script instead of
    /// the real `flatpak` binary.
    ///
    /// Unix-only: a `#!/bin/sh` script and its executable bit are a POSIX
    /// concept `std::fs::Permissions::from_mode` only exists to set: Sober
    /// itself is Linux-only (see this module's doc comment), so there is no
    /// real-world case to cover on Windows.
    #[cfg(unix)]
    #[test]
    fn is_installed_via_parses_a_matching_application_line() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let fake_flatpak = dir.join("flatpak");
        std::fs::write(&fake_flatpak, "#!/bin/sh\necho org.vinegarhq.Sober\n").unwrap();
        std::fs::set_permissions(&fake_flatpak, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(Sober::is_installed_via(fake_flatpak.to_str().unwrap()));
    }

    #[test]
    fn absence_error_distinguishes_installed_from_missing() {
        assert!(matches!(Sober::absence_error(true), SoberError::NeverRun));
        assert!(matches!(
            Sober::absence_error(false),
            SoberError::NotInstalled
        ));
    }

    #[test]
    fn extract_texture_reads_through_a_fixture_apk() {
        let data_dir = temp_dir();
        let apk_dir = data_dir.join("packages/x86_64/com.roblox.client");
        std::fs::create_dir_all(&apk_dir).unwrap();

        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file("assets/content/textures/face.png", options)
                .unwrap();
            std::io::Write::write_all(&mut writer, b"pngdata").unwrap();
            writer.finish().unwrap();
        }
        std::fs::write(apk_dir.join("base.apk"), &buf).unwrap();

        let sober = Sober::with_data_dir(data_dir);
        assert_eq!(
            sober.extract_texture("textures/face.png").unwrap(),
            b"pngdata"
        );
    }

    #[test]
    fn extract_texture_falls_back_to_extra_content_through_a_fixture_apk() {
        let data_dir = temp_dir();
        let apk_dir = data_dir.join("packages/x86_64/com.roblox.client");
        std::fs::create_dir_all(&apk_dir).unwrap();

        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file("assets/ExtraContent/textures/foo.png", options)
                .unwrap();
            std::io::Write::write_all(&mut writer, b"extra").unwrap();
            writer.finish().unwrap();
        }
        std::fs::write(apk_dir.join("base.apk"), &buf).unwrap();

        let sober = Sober::with_data_dir(data_dir);
        assert_eq!(sober.extract_texture("textures/foo.png").unwrap(), b"extra");
    }

    /// Manual-only smoke test against the real Sober install on this
    /// machine (verified present during development): confirms the file
    /// exists, decodes as PNG, and reports its size for a human to check.
    #[test]
    #[ignore = "reads the real ~/.var/app/org.vinegarhq.Sober install"]
    fn extract_texture_against_real_sober_install() {
        let sober = Sober::new().expect("HOME must be set");
        let bytes = sober
            .extract_texture("textures/face.png")
            .expect("Sober must be installed and have run once on this machine");
        eprintln!("textures/face.png: {} bytes", bytes.len());
        assert!(!bytes.is_empty());
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Png
        );
    }
}
