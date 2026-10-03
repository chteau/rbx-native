//! The loader's own tests; `names` and `store` keep theirs beside them.
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

/// A fresh, empty directory per call: tests run in parallel and must not
/// see each other's files.
fn scratch() -> std::path::PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rbx-native-script-templates-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, relative: &str, bytes: &[u8]) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn a_missing_directory_loads_as_nothing() {
    let templates = ScriptTemplates::load_from(&scratch().join("absent"));
    assert!(templates.skipped().is_empty());
    assert!(templates.extras().is_empty());
    assert_eq!(templates.default_for("Script"), None);
}

#[test]
fn a_file_is_an_extra_template_named_by_its_stem() {
    let dir = scratch();
    write(&dir, "ModuleScript/Enemy AI.luau", b"return {}\n");
    let templates = ScriptTemplates::load_from(&dir);
    assert_eq!(
        templates.extras(),
        [Template {
            class: "ModuleScript",
            name: "Enemy AI".into(),
            source: "return {}\n".into(),
        }]
    );
}

#[test]
fn default_luau_replaces_the_built_in_starter_and_is_not_listed() {
    let dir = scratch();
    write(&dir, "Script/Default.luau", b"print('mine')\n");
    let templates = ScriptTemplates::load_from(&dir);
    assert_eq!(templates.default_for("Script"), Some("print('mine')\n"));
    assert_eq!(templates.default_for("LocalScript"), None);
    assert!(templates.extras().is_empty());
}

#[test]
fn extras_follow_the_built_in_class_order_then_name_ignoring_case() {
    let dir = scratch();
    write(&dir, "ModuleScript/beta.luau", b"b");
    write(&dir, "Script/Zed.luau", b"z");
    write(&dir, "LocalScript/Mid.luau", b"m");
    write(&dir, "ModuleScript/Alpha.luau", b"a");
    let listed: Vec<_> = ScriptTemplates::load_from(&dir)
        .extras()
        .iter()
        .map(|t| (t.class, t.name.clone()))
        .collect();
    // Script, LocalScript, ModuleScript: the order of the three built-in
    // rows these are listed after, not alphabetical by class.
    assert_eq!(
        listed,
        [
            ("Script", "Zed".to_owned()),
            ("LocalScript", "Mid".to_owned()),
            ("ModuleScript", "Alpha".to_owned()),
            ("ModuleScript", "beta".to_owned()),
        ]
    );
}

#[test]
fn only_luau_files_in_a_script_class_folder_count() {
    let dir = scratch();
    write(&dir, "Script/notes.txt", b"x");
    write(&dir, "Script/readme", b"x");
    write(&dir, "Part/Thing.luau", b"x");
    write(&dir, "Thing.luau", b"x");
    assert!(ScriptTemplates::load_from(&dir).extras().is_empty());
}

#[test]
fn a_file_that_is_not_utf8_is_skipped_rather_than_failing_the_load() {
    let dir = scratch();
    write(&dir, "Script/Bad.luau", &[0xff, 0xfe, 0x00]);
    write(&dir, "Script/Good.luau", b"ok");
    let templates = ScriptTemplates::load_from(&dir);
    assert_eq!(templates.extras().len(), 1);
    assert_eq!(templates.extras()[0].name, "Good");
}

#[test]
fn a_file_over_the_size_limit_is_skipped() {
    let dir = scratch();
    write(
        &dir,
        "Script/Huge.luau",
        &vec![b'a'; MAX_BYTES as usize + 1],
    );
    assert!(ScriptTemplates::load_from(&dir).extras().is_empty());
}

#[test]
fn refused_files_are_listed_with_their_reason() {
    let dir = scratch();
    write(&dir, "Script/Notes.luau", &[0xff, 0xfe, 0x00]);
    write(&dir, "Script/Big.luau", &vec![b'a'; MAX_BYTES as usize + 1]);
    write(&dir, "Script/notes.txt", b"not a template");
    write(&dir, "LocalScript/Default.luau", &[0xff]);
    let skipped: Vec<_> = ScriptTemplates::load_from(&dir)
        .skipped()
        .iter()
        .map(|s| (s.class, s.file_name.clone(), s.reason))
        .collect();
    assert_eq!(
        skipped,
        [
            ("Script", "Big.luau".to_owned(), SkipReason::TooLarge),
            ("Script", "Notes.luau".to_owned(), SkipReason::NotUtf8),
            (
                "LocalScript",
                "Default.luau".to_owned(),
                SkipReason::NotUtf8
            ),
        ]
    );
}

#[test]
fn a_refused_file_says_why_in_one_line() {
    let dir = scratch();
    write(&dir, "Script/Big Generator.luau", &vec![b'a'; 312 * 1024]);
    write(&dir, "Script/Notes.luau", &[0xff]);
    let summaries: Vec<_> = ScriptTemplates::load_from(&dir)
        .skipped()
        .iter()
        .map(Skipped::summary)
        .collect();
    assert_eq!(
        summaries,
        ["312 KiB, over the 256 KiB limit", "Not UTF-8 text"]
    );
}
