use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::super::names::NameError;
use super::super::{ScriptTemplates, SkipReason, MAX_BYTES};
use super::{Imported, StoreError};

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rbx-native-template-store-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn read(dir: &Path, relative: &str) -> Option<String> {
    fs::read_to_string(dir.join(relative)).ok()
}

/// Every file under `dir`, relative and sorted: what a write left behind,
/// temp files included.
fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for class in fs::read_dir(dir).unwrap().flatten() {
        for file in fs::read_dir(class.path()).unwrap().flatten() {
            out.push(format!(
                "{}/{}",
                class.file_name().to_string_lossy(),
                file.file_name().to_string_lossy()
            ));
        }
    }
    out.sort();
    out
}

#[test]
fn create_writes_the_file_trimmed_and_refuses_a_clash() {
    let dir = scratch();
    let templates = ScriptTemplates::load_from(&dir);
    assert_eq!(
        templates
            .create("Script", "  Door  ", "print(1)\n")
            .unwrap(),
        "Door"
    );
    assert_eq!(
        read(&dir, "Script/Door.luau").as_deref(),
        Some("print(1)\n")
    );
    let templates = ScriptTemplates::load_from(&dir);
    assert!(matches!(
        templates.create("Script", "door", ""),
        Err(StoreError::Name(NameError::Taken { .. }))
    ));
    assert!(matches!(
        templates.create("Script", "Default", ""),
        Err(StoreError::Name(NameError::Reserved))
    ));
}

#[test]
fn write_replaces_atomically_and_leaves_no_temp_file() {
    let dir = scratch();
    let templates = ScriptTemplates::load_from(&dir);
    templates.write("Script", "Door", "one").unwrap();
    templates.write("Script", "Door", "two").unwrap();
    assert_eq!(read(&dir, "Script/Door.luau").as_deref(), Some("two"));
    assert_eq!(files(&dir), ["Script/Door.luau"]);
}

#[test]
fn a_write_over_the_limit_changes_nothing() {
    let dir = scratch();
    let templates = ScriptTemplates::load_from(&dir);
    templates.write("Script", "Door", "small").unwrap();
    let big = "a".repeat(MAX_BYTES as usize + 1);
    assert!(matches!(
        templates.write("Script", "Door", &big),
        Err(StoreError::TooLarge)
    ));
    assert_eq!(read(&dir, "Script/Door.luau").as_deref(), Some("small"));
}

#[test]
fn a_failed_write_keeps_the_last_good_file() {
    let dir = scratch();
    let templates = ScriptTemplates::load_from(&dir);
    templates.write("Script", "Door", "good").unwrap();
    // A directory where the temp file would go makes creating it fail.
    fs::create_dir(dir.join("Script/.Door.luau.tmp")).unwrap();
    assert!(matches!(
        templates.write("Script", "Door", "bad"),
        Err(StoreError::Io(_))
    ));
    assert_eq!(read(&dir, "Script/Door.luau").as_deref(), Some("good"));
}

#[test]
fn editing_the_starter_writes_default_and_deleting_it_resets() {
    let dir = scratch();
    let templates = ScriptTemplates::load_from(&dir);
    templates.write("LocalScript", "Default", "mine").unwrap();
    let templates = ScriptTemplates::load_from(&dir);
    assert_eq!(templates.default_for("LocalScript"), Some("mine"));
    templates.delete("LocalScript", "Default").unwrap();
    assert_eq!(
        ScriptTemplates::load_from(&dir).default_for("LocalScript"),
        None
    );
}

#[test]
fn rename_moves_the_file_and_may_change_only_the_casing() {
    let dir = scratch();
    ScriptTemplates::load_from(&dir)
        .write("Script", "Door", "x")
        .unwrap();
    ScriptTemplates::load_from(&dir)
        .write("Script", "Gate", "y")
        .unwrap();
    let templates = ScriptTemplates::load_from(&dir);
    assert!(matches!(
        templates.rename("Script", "Door", "gate"),
        Err(StoreError::Name(NameError::Taken { .. }))
    ));
    assert_eq!(templates.rename("Script", "Door", "DOOR").unwrap(), "DOOR");
    let templates = ScriptTemplates::load_from(&dir);
    assert_eq!(
        templates.rename("Script", "DOOR", "Hatch ").unwrap(),
        "Hatch"
    );
    assert_eq!(files(&dir), ["Script/Gate.luau", "Script/Hatch.luau"]);
}

#[test]
fn moving_to_another_class_applies_that_class_s_clash_rule() {
    let dir = scratch();
    let templates = ScriptTemplates::load_from(&dir);
    templates.write("Script", "Shared", "s").unwrap();
    templates.write("Script", "Solo", "o").unwrap();
    templates.write("ModuleScript", "Shared", "m").unwrap();
    let templates = ScriptTemplates::load_from(&dir);
    assert!(matches!(
        templates.move_to("Script", "Shared", "ModuleScript"),
        Err(StoreError::Name(NameError::Taken {
            class: "ModuleScript",
            ..
        }))
    ));
    templates.move_to("Script", "Solo", "ModuleScript").unwrap();
    assert_eq!(read(&dir, "ModuleScript/Solo.luau").as_deref(), Some("o"));
    assert_eq!(read(&dir, "Script/Solo.luau"), None);
}

#[test]
fn delete_and_delete_skipped_remove_the_file() {
    let dir = scratch();
    let templates = ScriptTemplates::load_from(&dir);
    templates.write("Script", "Door", "x").unwrap();
    templates.delete("Script", "Door").unwrap();
    fs::write(dir.join("Script/Notes.luau"), [0xff, 0xfe]).unwrap();
    let templates = ScriptTemplates::load_from(&dir);
    assert_eq!(templates.skipped().len(), 1);
    templates.delete_skipped("Script", "Notes.luau").unwrap();
    assert!(files(&dir).is_empty());
}

#[test]
fn import_copies_luau_files_renaming_clashes_and_reports_the_rest() {
    let dir = scratch();
    let outside = scratch();
    let templates = ScriptTemplates::load_from(&dir);
    templates.write("Script", "Door", "old").unwrap();
    let pick = |name: &str, bytes: &[u8]| {
        let path = outside.join(name);
        fs::write(&path, bytes).unwrap();
        path
    };
    let picked = [
        pick("Door.luau", b"new"),
        pick("readme.md", b"x"),
        pick("Bad.luau", &[0xff]),
        pick("Default.luau", b"d"),
        pick("door.luau", b"newer"),
    ];
    let imported = ScriptTemplates::load_from(&dir)
        .import("LocalScript", &picked[..1])
        .unwrap();
    assert_eq!(imported.added, ["Door"]);
    let imported = ScriptTemplates::load_from(&dir)
        .import("Script", &picked)
        .unwrap();
    assert_eq!(
        imported,
        Imported {
            added: vec!["Door 2".into(), "Default 2".into(), "door 3".into()],
            ignored: 1,
            refused: vec![("Bad.luau".into(), SkipReason::NotUtf8)],
        }
    );
    assert_eq!(read(&dir, "Script/Door.luau").as_deref(), Some("old"));
    assert_eq!(read(&dir, "Script/door 3.luau").as_deref(), Some("newer"));
}

#[test]
fn without_a_config_directory_every_write_says_so() {
    let templates = ScriptTemplates::default();
    assert!(matches!(
        templates.create("Script", "Door", ""),
        Err(StoreError::NoDir)
    ));
}
