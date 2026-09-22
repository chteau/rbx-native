use super::*;

fn files(pairs: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
    pairs
        .iter()
        .map(|(path, content)| (path.to_string(), content.as_bytes().to_vec()))
        .collect()
}

#[test]
fn a_single_init_file_at_the_root_makes_the_package_a_module_script_directly() {
    // `sleitnick/net`'s real shape, confirmed by hand this session:
    // wally.toml + init.lua, nothing else.
    let node = build(
        &files(&[("init.lua", "return {}"), ("wally.toml", "...")]),
        "net",
    );
    assert_eq!(node.name, "net");
    assert_eq!(node.source.as_deref(), Some("return {}"));
    assert!(node.children.is_empty(), "wally.toml is not Lua, skipped");
}

#[test]
fn a_plain_lua_file_becomes_a_leaf_module_script_named_without_its_extension() {
    let node = build(
        &files(&[("init.lua", "return {}"), ("Sub.lua", "return 1")]),
        "pkg",
    );
    assert_eq!(node.children.len(), 1);
    assert_eq!(node.children[0].name, "Sub");
    assert_eq!(node.children[0].source.as_deref(), Some("return 1"));
    assert!(node.children[0].children.is_empty());
}

#[test]
fn luau_extension_is_recognized_too() {
    let node = build(&files(&[("init.luau", "return {}")]), "pkg");
    assert_eq!(node.source.as_deref(), Some("return {}"));
}

#[test]
fn a_subdirectory_with_no_init_file_becomes_a_plain_folder() {
    let node = build(
        &files(&[("init.lua", "return {}"), ("modules/util.lua", "return 2")]),
        "pkg",
    );
    let modules = node
        .children
        .iter()
        .find(|c| c.name == "modules")
        .expect("modules folder");
    assert!(modules.source.is_none(), "no init file, so it's a Folder");
    assert_eq!(modules.children.len(), 1);
    assert_eq!(modules.children[0].name, "util");
}

#[test]
fn a_subdirectory_with_its_own_init_file_becomes_a_nested_module_script() {
    let node = build(
        &files(&[
            ("init.lua", "return {}"),
            ("Sub/init.lua", "return 3"),
            ("Sub/Helper.lua", "return 4"),
        ]),
        "pkg",
    );
    let sub = node.children.iter().find(|c| c.name == "Sub").unwrap();
    assert_eq!(sub.source.as_deref(), Some("return 3"));
    assert_eq!(sub.children.len(), 1);
    assert_eq!(sub.children[0].name, "Helper");
}

#[test]
fn non_lua_files_are_skipped_entirely_not_turned_into_placeholder_instances() {
    let node = build(
        &files(&[
            ("init.lua", "return {}"),
            ("README.md", "# hi"),
            ("moonwave.toml", "..."),
        ]),
        "pkg",
    );
    assert!(node.children.is_empty());
}

#[test]
fn a_root_with_no_init_file_at_all_becomes_a_folder() {
    let node = build(&files(&[("Loose.lua", "return 5")]), "pkg");
    assert!(node.source.is_none());
    assert_eq!(node.children.len(), 1);
    assert_eq!(node.children[0].name, "Loose");
}

#[test]
fn init_luau_is_preferred_over_init_lua_when_somehow_both_are_present() {
    let node = build(
        &files(&[("init.lua", "lua version"), ("init.luau", "luau version")]),
        "pkg",
    );
    assert_eq!(node.source.as_deref(), Some("luau version"));
}
