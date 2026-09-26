use std::io::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};

use zip::write::SimpleFileOptions;

use super::*;

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rbx-native-theme-github-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (path, bytes) in files {
        writer.start_file(*path, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

const MANIFEST: &[u8] = br#"{"name":"Dusk","author":"@someone","description":"Warm.","version":"1.0.0","preview":"preview.png"}"#;

fn source(owner: &str, repo: &str, reference: Option<&str>) -> Source {
    Source {
        owner: owner.into(),
        repo: repo.into(),
        reference: reference.map(Into::into),
    }
}

#[test]
fn links_in_the_shapes_people_paste_them_parse() {
    for link in [
        "https://github.com/someone/dusk-theme",
        "http://github.com/someone/dusk-theme/",
        "github.com/someone/dusk-theme",
        "https://www.github.com/someone/dusk-theme.git",
        "  https://github.com/someone/dusk-theme?tab=readme#top ",
    ] {
        assert_eq!(
            Source::parse(link),
            Ok(source("someone", "dusk-theme", None)),
            "{link}"
        );
    }
    assert_eq!(
        Source::parse("https://github.com/someone/dusk-theme/tree/v2/night"),
        Ok(source("someone", "dusk-theme", Some("v2/night")))
    );
}

#[test]
fn anything_but_a_repository_link_is_refused() {
    for link in [
        "",
        "https://gitlab.com/someone/dusk",
        "https://github.com/someone",
        "https://github.com/someone/dusk/blob/main/theme.json",
        "https://github.com/someone/dusk/tree/",
        "https://github.com/../dusk",
        "https://github.com/someone/du sk",
        "https://github.com/someone/dusk/tree/../../x",
        "https://evil.example/github.com/someone/dusk",
    ] {
        assert!(Source::parse(link).is_err(), "{link:?}");
    }
}

#[test]
fn the_archive_url_defaults_to_the_default_branch() {
    assert_eq!(
        source("a", "b", None).archive_url(),
        "https://github.com/a/b/archive/HEAD.zip"
    );
    assert_eq!(
        source("a", "b", Some("v1")).archive_url(),
        "https://github.com/a/b/archive/v1.zip"
    );
}

#[test]
fn a_repository_named_default_cannot_take_the_built_in_themes_name() {
    assert_eq!(source("someone", "dusk", None).id(), "dusk");
    assert_eq!(source("someone", "Default", None).id(), "someone-Default");
}

#[test]
fn a_github_archive_installs_its_wrapped_folder_and_nothing_else() {
    let themes = scratch();
    let archive = zip(&[
        ("dusk-main/manifest.json", MANIFEST),
        ("dusk-main/preview.png", b"png"),
        (
            "dusk-main/theme.json",
            br##"{"colors":{"dock":"#201810"}}"##,
        ),
        ("dusk-main/icons/Part.svg", b"<svg/>"),
        ("dusk-main/examples/manifest.json", b"not the theme"),
    ]);

    let manifest = install_archive(&themes, "dusk", &archive).unwrap();
    assert_eq!(manifest.name, "Dusk");
    assert_eq!(manifest.author, "@someone");

    let dir = themes.join("dusk");
    assert!(dir.join("manifest.json").is_file());
    assert!(dir.join("icons/Part.svg").is_file());
    assert!(!themes.join("dusk-main").exists());
    let pack = ThemePack::load_from(&themes, "dusk").unwrap();
    assert!(pack.icons.is_some());
    // Only the installed folder is left: no staging or backup folders.
    assert_eq!(fs::read_dir(&themes).unwrap().count(), 1);
}

#[test]
fn a_hand_made_zip_with_the_manifest_at_its_root_installs_too() {
    let themes = scratch();
    let archive = zip(&[("manifest.json", MANIFEST), ("preview.png", b"png")]);
    install_archive(&themes, "dusk", &archive).unwrap();
    assert!(themes.join("dusk/preview.png").is_file());
}

#[test]
fn an_invalid_download_leaves_the_installed_theme_untouched() {
    let themes = scratch();
    install_archive(
        &themes,
        "dusk",
        &zip(&[("r/manifest.json", MANIFEST), ("r/preview.png", b"v1")]),
    )
    .unwrap();

    // No preview image: the manifest does not validate.
    let broken = zip(&[("r/manifest.json", MANIFEST)]);
    assert!(install_archive(&themes, "dusk", &broken).is_err());
    // A bad colour in theme.json fails the same way.
    let bad_colour = zip(&[
        ("r/manifest.json", MANIFEST),
        ("r/preview.png", b"v2"),
        ("r/theme.json", br#"{"colors":{"dock":"orange"}}"#),
    ]);
    assert!(install_archive(&themes, "dusk", &bad_colour)
        .unwrap_err()
        .contains("dock"));
    assert!(install_archive(&themes, "dusk", &zip(&[("README.md", b"hi")])).is_err());
    assert!(install_archive(&themes, "dusk", b"not a zip").is_err());

    assert_eq!(fs::read(themes.join("dusk/preview.png")).unwrap(), b"v1");
    assert_eq!(fs::read_dir(&themes).unwrap().count(), 1);
}

#[test]
fn reinstalling_replaces_the_previous_version() {
    let themes = scratch();
    let version = |preview: &[u8], extra: Option<&str>| {
        let mut files: Vec<(&str, &[u8])> =
            vec![("r/manifest.json", MANIFEST), ("r/preview.png", preview)];
        if let Some(extra) = extra {
            files.push((extra, b"x"));
        }
        zip(&files)
    };
    install_archive(&themes, "dusk", &version(b"v1", Some("r/icons/Old.svg"))).unwrap();
    install_archive(&themes, "dusk", &version(b"v2", None)).unwrap();
    assert_eq!(fs::read(themes.join("dusk/preview.png")).unwrap(), b"v2");
    assert!(!themes.join("dusk/icons/Old.svg").exists());
}

#[test]
fn paths_that_climb_out_of_the_archive_are_never_written() {
    let themes = scratch();
    let archive = zip(&[
        ("r/manifest.json", MANIFEST),
        ("r/preview.png", b"png"),
        ("r/../../escaped.txt", b"x"),
        ("/abs.txt", b"x"),
    ]);
    install_archive(&themes, "dusk", &archive).unwrap();
    assert!(!themes.join("escaped.txt").exists());
    assert!(!themes.parent().unwrap().join("escaped.txt").exists());
}

/// Downloads a real, tiny repository that is not a theme, end to end
/// through GitHub's archive redirect: `cargo test -- --ignored`.
#[test]
#[ignore = "needs the network"]
fn a_real_repository_without_a_manifest_is_refused_after_downloading() {
    let themes = scratch();
    let err = install_into(&themes, "https://github.com/octocat/Hello-World").unwrap_err();
    assert!(err.contains("no manifest.json"), "{err}");
}
