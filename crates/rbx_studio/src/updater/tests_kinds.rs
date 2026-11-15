//! The install kinds beyond an unpacked folder (an AppImage, a Linux
//! install still named the way 0.1.0 shipped it) and the install scripts.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use super::install::{self, Location};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbx-updater-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn os(path: &Path) -> Option<OsString> {
    Some(path.as_os_str().to_owned())
}

#[test]
fn an_appimage_is_found_only_from_inside_its_mount() {
    let dir = scratch("appimage-env");
    let image = dir.join("RbxNative-0.2.0-x86_64.AppImage");
    fs::write(&image, "image").unwrap();
    let mount = Path::new("/tmp/.mount_RbxNatXYZ");
    let exe = mount.join("usr/bin/rbxnative");

    assert_eq!(
        install::appimage_of(&exe, os(&image), os(mount)),
        Some(image.clone())
    );
    // Started by some other AppImage: the variables are its, not ours.
    let elsewhere = Path::new("/home/x/.local/share/rbxnative/rbxnative");
    assert_eq!(install::appimage_of(elsewhere, os(&image), os(mount)), None);
    assert_eq!(install::appimage_of(&exe, None, os(mount)), None);
    assert_eq!(install::appimage_of(&exe, os(&image), None), None);
    assert_eq!(
        install::appimage_of(&exe, os(&dir.join("gone.AppImage")), os(mount)),
        None
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_appimage_is_replaced_by_the_downloaded_one() {
    let dir = scratch("appimage");
    let image = dir.join("RbxNative.AppImage");
    fs::write(&image, "old").unwrap();
    let staging = install::Staging::new(&dir).unwrap();
    let download = staging.path().join("RbxNative-0.2.0-x86_64.AppImage");
    fs::write(&download, "new").unwrap();

    let location = Location::AppImage(image.clone());
    assert_eq!(location.dir(), dir);
    location.install(&download, staging.path()).unwrap();
    assert_eq!(fs::read_to_string(&image).unwrap(), "new");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&image).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    drop(staging);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_legacy_named_install_takes_the_new_name_and_keeps_a_link() {
    use super::compat::migrate_legacy_name;
    use super::release::EXE_NAME;

    let dir = scratch("legacy");
    let legacy = dir.join("rbxstudio");
    fs::write(&legacy, "editor").unwrap();
    let new = migrate_legacy_name(&legacy).unwrap();
    assert_eq!(new, dir.join(EXE_NAME));
    assert_eq!(fs::read_to_string(&new).unwrap(), "editor");
    assert_eq!(fs::read_link(&legacy).unwrap(), PathBuf::from(EXE_NAME));
    assert_eq!(fs::read_to_string(&legacy).unwrap(), "editor");
    // Done once: the old name is a link now.
    assert_eq!(migrate_legacy_name(&legacy), None);
    // Already the new name, or something else: left alone.
    assert_eq!(migrate_legacy_name(&new), None);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_legacy_name_beside_a_new_one_is_left_alone() {
    use super::release::EXE_NAME;

    let dir = scratch("legacy-both");
    let legacy = dir.join("rbxstudio");
    fs::write(&legacy, "old").unwrap();
    fs::write(dir.join(EXE_NAME), "new").unwrap();
    assert_eq!(super::compat::migrate_legacy_name(&legacy), None);
    assert_eq!(fs::read_to_string(&legacy).unwrap(), "old");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_install_scripts_trust_the_release_key() {
    for script in [
        include_str!("../../../../packaging/install/install.sh"),
        include_str!("../../../../packaging/install/install.ps1"),
    ] {
        assert!(script.contains(super::verify::PUBLIC_KEY));
    }
}
