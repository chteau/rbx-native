use std::fs;
use std::path::PathBuf;

use lsp_types::FileChangeType;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Value};

use super::{Mirror, SOURCEMAP};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rbx-luau-mirror-{}-{name}", std::process::id()))
}

fn place() -> (WeakDom, Ref, Ref) {
    let mut dom = WeakDom::new();
    let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
    let module = dom.new_instance("ModuleScript", "Mod", Some(storage));
    let _ = dom.set_property(module, "Source", Variant::String("return 1".into()));
    let _ = dom.new_instance("Folder", "Assets", Some(storage));
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let script = dom.new_instance("Script", "Main", Some(workspace));
    (dom, module, script)
}

fn read_sourcemap(mirror: &Mirror) -> Value {
    serde_json::from_str(&fs::read_to_string(mirror.root().join(SOURCEMAP)).unwrap()).unwrap()
}

#[test]
fn writes_every_script_and_a_rojo_shaped_sourcemap() {
    let (dom, module, script) = place();
    let db = ReflectionDatabase::embedded();
    let mut mirror = Mirror::new(scratch("first"));
    let changes = mirror.sync(&dom, &db).unwrap();

    assert_eq!(
        changes.len(),
        3,
        "two scripts and the sourcemap: {changes:?}"
    );
    assert_eq!(
        fs::read_to_string(mirror.path_of(module)).unwrap(),
        "return 1"
    );
    // No `Source` yet reads as an empty script, not a missing one.
    assert_eq!(fs::read_to_string(mirror.path_of(script)).unwrap(), "");

    let module_file = format!("{}.luau", module.value());
    assert_eq!(
        read_sourcemap(&mirror)["children"][0],
        json!({
            "name": "ReplicatedStorage",
            "className": "ReplicatedStorage",
            "children": [
                {"name": "Mod", "className": "ModuleScript", "filePaths": [module_file]},
                {"name": "Assets", "className": "Folder"},
            ],
        })
    );
    assert_eq!(read_sourcemap(&mirror)["className"], "DataModel");
}

#[test]
fn a_second_sync_reports_only_what_moved() {
    let (mut dom, module, script) = place();
    let db = ReflectionDatabase::embedded();
    let mut mirror = Mirror::new(scratch("second"));
    mirror.sync(&dom, &db).unwrap();
    assert!(mirror.sync(&dom, &db).unwrap().is_empty());

    let _ = dom.set_property(module, "Source", Variant::String("return 2".into()));
    assert_eq!(
        mirror.sync(&dom, &db).unwrap(),
        vec![(mirror.path_of(module), FileChangeType::CHANGED)]
    );

    // A rename leaves every script's text alone and only moves the map.
    dom.set_name(script, "Renamed").unwrap();
    assert_eq!(
        mirror.sync(&dom, &db).unwrap(),
        vec![(mirror.root().join(SOURCEMAP), FileChangeType::CHANGED)]
    );

    dom.remove(script);
    let changes = mirror.sync(&dom, &db).unwrap();
    assert!(changes.contains(&(mirror.path_of(script), FileChangeType::DELETED)));
    assert!(!mirror.path_of(script).exists());
}

#[test]
fn maps_a_file_back_to_its_script_and_nothing_else() {
    let (dom, module, _) = place();
    let db = ReflectionDatabase::embedded();
    let mut mirror = Mirror::new(scratch("lookup"));
    mirror.sync(&dom, &db).unwrap();

    assert_eq!(mirror.script_at(&mirror.path_of(module)), Some(module));
    assert_eq!(mirror.script_at(&mirror.root().join(SOURCEMAP)), None);
    assert_eq!(mirror.script_at(&mirror.root().join("999999.luau")), None);
}

#[test]
fn dropping_the_mirror_removes_its_folder() {
    let (dom, _, _) = place();
    let db = ReflectionDatabase::embedded();
    let root = scratch("drop");
    let mut mirror = Mirror::new(root.clone());
    mirror.sync(&dom, &db).unwrap();
    drop(mirror);
    assert!(!root.exists());
}
