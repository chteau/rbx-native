use std::fs;
use std::path::{Path, PathBuf};

use semver::Version;
use sha2::{Digest, Sha256};

use super::install::{self, Location};
use super::release::{self, Release};
use super::{verify, Prefs};

/// A test-only key (not the release key) and what it signed: the
/// `SHA256SUMS` in `fixtures/`, listing `sha256("rbxstudio payload")` for the
/// Linux package and `sha256("windows payload")` for the Windows one.
const TEST_KEY: &str = "RWQQSfE791LIvlPN9R68BfsoDohUV236Ys1QAEf/wstld16Mr6HcKLT/";
const SUMS: &str = include_str!("fixtures/SHA256SUMS");
const SIGNATURE: &str = include_str!("fixtures/SHA256SUMS.minisig");
const LINUX_PACKAGE: &str = "rbxstudio-0.2.0-x86_64-unknown-linux-gnu.tar.gz";

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbx-updater-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn release(tag: &str, prerelease: bool) -> Release {
    Release::parse(&format!(
        r#"{{"tag_name":"{tag}","html_url":"https://github.com/chteau/rbx-native/releases/tag/{tag}",
            "prerelease":{prerelease},"draft":false,"assets":[
            {{"name":"rbxstudio-0.2.0-x86_64-unknown-linux-gnu.tar.gz","browser_download_url":"https://x/linux"}},
            {{"name":"rbxstudio-0.2.0-aarch64-apple-darwin.tar.gz","browser_download_url":"https://x/mac"}},
            {{"name":"rbxstudio-0.2.0-x86_64-pc-windows-msvc.zip","browser_download_url":"https://x/win"}},
            {{"name":"SHA256SUMS","browser_download_url":"https://x/sums"}},
            {{"name":"SHA256SUMS.minisig","browser_download_url":"https://x/sig"}}]}}"#
    ))
    .unwrap()
}

#[test]
fn versions_compare_as_semver() {
    let current = v("0.1.0");
    assert!(release("v0.2.0", false).is_newer_than(&current).unwrap());
    assert!(release("0.1.1", false).is_newer_than(&current).unwrap());
    assert!(release("v0.10.0", false)
        .is_newer_than(&v("0.9.0"))
        .unwrap());
    assert!(!release("v0.1.0", false).is_newer_than(&current).unwrap());
    assert!(!release("v0.0.9", false).is_newer_than(&current).unwrap());
    assert!(!release("v0.2.0-beta.1", false)
        .is_newer_than(&current)
        .unwrap());
    assert!(!release("v0.2.0", true).is_newer_than(&current).unwrap());
    assert!(release("nightly", false).is_newer_than(&current).is_err());
}

#[test]
fn the_package_for_each_target_is_picked_by_name() {
    let release = release("v0.2.0", false);
    let version = v("0.2.0");
    let pick = |target| {
        release
            .package(&version, target)
            .unwrap()
            .browser_download_url
            .clone()
    };
    assert_eq!(pick("x86_64-unknown-linux-gnu"), "https://x/linux");
    assert_eq!(pick("aarch64-apple-darwin"), "https://x/mac");
    assert_eq!(pick("x86_64-pc-windows-msvc"), "https://x/win");
    assert!(release
        .package(&version, "riscv64gc-unknown-linux-gnu")
        .is_err());
    assert!(release
        .package(&v("0.3.0"), "x86_64-unknown-linux-gnu")
        .is_err());
    assert_eq!(
        release::package_name(&version, "x86_64-pc-windows-msvc"),
        "rbxstudio-0.2.0-x86_64-pc-windows-msvc.zip"
    );
}

#[test]
fn a_good_signature_verifies() {
    verify::signature(TEST_KEY, SUMS.as_bytes(), SIGNATURE).unwrap();
}

#[test]
fn tampered_sums_are_refused() {
    let tampered = SUMS.replacen("2459fb", "2459fc", 1);
    assert!(verify::signature(TEST_KEY, tampered.as_bytes(), SIGNATURE).is_err());
    let appended = format!("{SUMS}deadbeef  evil.tar.gz\n");
    assert!(verify::signature(TEST_KEY, appended.as_bytes(), SIGNATURE).is_err());
}

#[test]
fn another_key_is_refused() {
    assert!(verify::signature(verify::PUBLIC_KEY, SUMS.as_bytes(), SIGNATURE).is_err());
    assert!(verify::signature(TEST_KEY, SUMS.as_bytes(), "not a signature").is_err());
}

#[test]
fn the_release_key_parses() {
    assert!(minisign_verify::PublicKey::from_base64(verify::PUBLIC_KEY).is_ok());
}

#[test]
fn a_package_must_match_its_listed_sum() {
    let listed = verify::sum_for(SUMS, LINUX_PACKAGE).unwrap();
    let hash = |bytes: &[u8]| verify::hex(&Sha256::digest(bytes));
    assert_eq!(hash(b"rbxstudio payload"), listed);
    assert_ne!(hash(b"rbxstudio payload!"), listed);
    assert!(verify::sum_for(SUMS, "rbxstudio-0.2.0-aarch64-apple-darwin.tar.gz").is_err());
    assert!(verify::sum_for("abc  x.tar.gz\n", "x.tar.gz").is_err());
    let binary_mode = format!("{listed} *{LINUX_PACKAGE}\n");
    assert_eq!(
        verify::sum_for(&binary_mode, LINUX_PACKAGE).unwrap(),
        listed
    );
}

#[test]
fn the_windows_dance_moves_the_running_exe_aside() {
    let dir = scratch("windows");
    let exe = dir.join("rbxstudio.exe");
    let new = dir.join("staged.exe");
    fs::write(&exe, "old").unwrap();
    fs::write(&new, "new").unwrap();
    let old = install::old_exe(&exe);
    assert_eq!(old, dir.join("rbxstudio.old.exe"));
    install::swap_aside(&new, &exe, &old).unwrap();
    assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
    assert_eq!(fs::read_to_string(&old).unwrap(), "old");
    assert!(!new.exists());

    // A second update replaces the leftover from the first.
    fs::write(&new, "newer").unwrap();
    install::swap_aside(&new, &exe, &old).unwrap();
    assert_eq!(fs::read_to_string(&exe).unwrap(), "newer");
    assert_eq!(fs::read_to_string(&old).unwrap(), "new");

    // The next launch deletes it.
    Location::of(&exe).clear_leftovers();
    assert!(!old.exists());
    assert!(exe.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_swapped_out_executable_still_names_the_install() {
    let path = |p: &str| install::running_path(PathBuf::from(p));
    assert_eq!(
        path("/opt/rbx/rbxstudio (deleted)"),
        PathBuf::from("/opt/rbx/rbxstudio")
    );
    assert_eq!(
        path("C:/rbx/rbxstudio.old.exe"),
        PathBuf::from("C:/rbx/rbxstudio.exe")
    );
    assert_eq!(
        path("/opt/rbx/rbxstudio"),
        PathBuf::from("/opt/rbx/rbxstudio")
    );
}

#[test]
fn a_failed_swap_puts_the_running_exe_back() {
    let dir = scratch("rollback");
    let exe = dir.join("rbxstudio.exe");
    fs::write(&exe, "old").unwrap();
    let missing = dir.join("never-downloaded.exe");
    assert!(install::swap_aside(&missing, &exe, &install::old_exe(&exe)).is_err());
    assert_eq!(fs::read_to_string(&exe).unwrap(), "old");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unix_renames_over_the_running_binary() {
    let dir = scratch("unix");
    let exe = dir.join("rbxstudio");
    let new = dir.join("staged");
    fs::write(&exe, "old").unwrap();
    fs::write(&new, "new").unwrap();
    install::swap_over(&new, &exe).unwrap();
    assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

fn tar_gz(files: &[(&str, &[u8])], out: &Path) {
    let gz =
        flate2::write::GzEncoder::new(fs::File::create(out).unwrap(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(gz);
    for (path, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, path, *bytes).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap();
}

#[test]
fn a_package_replaces_a_bare_binary() {
    let dir = scratch("package");
    let exe = dir.join("rbxstudio-renamed");
    fs::write(&exe, "old").unwrap();
    let archive = dir.join("pkg.tar.gz");
    let inner = format!("rbxstudio-0.2.0-x/{}", release::EXE_NAME);
    tar_gz(
        &[("rbxstudio-0.2.0-x/LICENSE", b"MIT"), (&inner, b"new")],
        &archive,
    );
    let unpacked = dir.join("unpacked");
    install::unpack(&archive, &unpacked).unwrap();
    Location::of(&exe).replace_from(&unpacked).unwrap();
    assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_package_replaces_a_whole_app_bundle() {
    let dir = scratch("bundle");
    let bundle = dir.join("RbxNative.app");
    let exe = bundle.join("Contents/MacOS/rbxstudio");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, "old").unwrap();
    fs::write(bundle.join("Contents/Info.plist"), "old plist").unwrap();
    let location = Location::of(&exe);
    assert_eq!(location, Location::Bundle(bundle.clone()));
    assert_eq!(location.dir(), dir);

    let archive = dir.join("pkg.tar.gz");
    tar_gz(
        &[
            (
                "rbxstudio-0.2.0-aarch64-apple-darwin/RbxNative.app/Contents/Info.plist",
                b"new plist",
            ),
            (
                "rbxstudio-0.2.0-aarch64-apple-darwin/RbxNative.app/Contents/MacOS/rbxstudio",
                b"new",
            ),
        ],
        &archive,
    );
    let unpacked = dir.join("unpacked");
    install::unpack(&archive, &unpacked).unwrap();
    location.replace_from(&unpacked).unwrap();
    assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
    assert_eq!(
        fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap(),
        "new plist"
    );
    assert!(!dir.join("RbxNative.app.old").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_zip_package_unpacks() {
    use std::io::Write;
    let dir = scratch("zip");
    let archive = dir.join("pkg.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    zip.start_file(
        "rbxstudio-0.2.0-x86_64-pc-windows-msvc/rbxstudio.exe",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"new").unwrap();
    zip.finish().unwrap();
    let unpacked = dir.join("unpacked");
    install::unpack(&archive, &unpacked).unwrap();
    assert_eq!(
        fs::read_to_string(unpacked.join("rbxstudio-0.2.0-x86_64-pc-windows-msvc/rbxstudio.exe"))
            .unwrap(),
        "new"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn flatpak_is_detected_by_id_or_info_file() {
    let dir = scratch("flatpak");
    let info = dir.join(".flatpak-info");
    assert!(!install::in_flatpak(None, &info));
    assert!(!install::in_flatpak(Some("".into()), &info));
    assert!(install::in_flatpak(
        Some("io.github.chteau.RbxNative".into()),
        &info
    ));
    fs::write(&info, "[Application]").unwrap();
    assert!(install::in_flatpak(None, &info));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_unwritable_folder_is_detected() {
    let dir = scratch("writable");
    assert!(install::writable(&dir));
    assert!(!install::writable(&dir.join("does-not-exist")));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        // root ignores the mode bits; the check is still right for it.
        let root = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join("probe"))
            .is_ok();
        assert_eq!(install::writable(&dir), root);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(
        fs::read_dir(&dir)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name() != "probe")
            .count(),
        0
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prefs_default_to_automatic() {
    let prefs: Prefs = serde_json::from_str("{}").unwrap();
    assert!(prefs.automatic);
    assert_eq!(prefs, Prefs::default());
    let off: Prefs = serde_json::from_str(r#"{"automatic":false}"#).unwrap();
    assert!(!off.automatic);
}
