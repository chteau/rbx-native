use std::path::PathBuf;

use super::{fonts, FontsFile, MAX_FONT_BYTES};

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbx-fonts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn file(ui: Option<&str>, mono: Option<&str>, files: &[&str]) -> FontsFile {
    FontsFile {
        ui: ui.map(str::to_string),
        mono: mono.map(str::to_string),
        files: files.iter().map(|f| f.to_string()).collect(),
    }
}

#[test]
fn absent_fonts_are_default() {
    let f = fonts(None, None).unwrap();
    assert!(f.ui.is_none() && f.mono.is_none() && f.files.is_empty());
    let f = fonts(Some(&file(None, None, &[])), None).unwrap();
    assert!(f.ui.is_none() && f.mono.is_none() && f.files.is_empty());
}

#[test]
fn families_need_no_folder() {
    let f = fonts(Some(&file(Some(" Inter "), Some("Fira Code"), &[])), None).unwrap();
    assert_eq!(f.ui.as_deref(), Some("Inter"));
    assert_eq!(f.mono.as_deref(), Some("Fira Code"));
}

#[test]
fn blank_or_long_family_is_rejected() {
    assert!(fonts(Some(&file(Some("  "), None, &[])), None).is_err());
    assert!(fonts(Some(&file(None, Some(&"x".repeat(65)), &[])), None).is_err());
}

#[test]
fn files_without_a_folder_are_rejected() {
    assert!(fonts(Some(&file(None, None, &["a.ttf"])), None).is_err());
}

#[test]
fn good_file_resolves() {
    let dir = tempdir("good");
    std::fs::write(dir.join("A.TTF"), b"font").unwrap();
    let f = fonts(Some(&file(None, None, &["A.TTF"])), Some(&dir)).unwrap();
    assert_eq!(f.files.len(), 1);
    assert!(f.files[0].ends_with("A.TTF"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_files_are_rejected() {
    let dir = tempdir("bad");
    std::fs::write(dir.join("a.txt"), b"x").unwrap();
    let outside = dir.with_extension("outside.ttf");
    std::fs::write(&outside, b"x").unwrap();
    let escape = format!("../{}", outside.file_name().unwrap().to_str().unwrap());
    let big = std::fs::File::create(dir.join("big.ttf")).unwrap();
    big.set_len(MAX_FONT_BYTES + 1).unwrap();
    for name in [escape.as_str(), "a.txt", "missing.ttf", "big.ttf"] {
        assert!(
            fonts(Some(&file(None, None, &[name])), Some(&dir)).is_err(),
            "{name}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&outside);
}

#[test]
fn too_many_files_are_rejected() {
    let dir = tempdir("many");
    let names: Vec<String> = (0..17).map(|i| format!("f{i}.ttf")).collect();
    for name in &names {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    assert!(fonts(Some(&file(None, None, &refs)), Some(&dir)).is_err());
    assert!(fonts(Some(&file(None, None, &refs[..16])), Some(&dir)).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}
