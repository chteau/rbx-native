// `Shell::wally_install`/`apply_wally_graph`'s own `cx.spawn` plumbing needs
// a live GPUI `Context<Shell>` this crate has no headless harness for, the
// same boundary `shell::argon_sync`'s own tests stop at. What's tested here
// is the pure logic underneath: the alias-text formatters, and the DOM
// find-or-create/realize helpers against a plain `WeakDom`.

use rbx_dom::WeakDom;

use crate::wally_client::PackageNode;

use super::*;

#[test]
fn require_sibling_reaches_a_slot_next_to_this_ones_parent() {
    assert_eq!(
        require_sibling("sleitnick_comm@1.0.1", "comm"),
        r#"return require(script.Parent.Parent["sleitnick_comm@1.0.1"]["comm"])"#
    );
}

#[test]
fn require_index_reaches_a_slot_under_this_roots_own_index() {
    assert_eq!(
        require_index("sleitnick_net@0.2.0", "net"),
        r#"return require(script.Parent._Index["sleitnick_net@0.2.0"]["net"])"#
    );
}

#[test]
fn lua_string_escapes_quotes_and_backslashes() {
    assert_eq!(lua_string("plain"), "\"plain\"");
    assert_eq!(lua_string(r#"a"b\c"#), r#""a\"b\\c""#);
}

#[test]
fn slot_name_from_key_matches_slot_names_own_format() {
    let key: PackageKey = (
        "sleitnick".to_owned(),
        "net".to_owned(),
        semver::Version::parse("0.2.0").unwrap(),
    );
    assert_eq!(slot_name_from_key(&key), "sleitnick_net@0.2.0");
}

#[test]
fn find_or_create_root_makes_the_right_service_per_realm() {
    let mut dom = WeakDom::new();
    let packages = find_or_create_root(&mut dom, Realm::Shared);
    assert_eq!(dom.get(packages).unwrap().class(), "Packages");
    let server_packages = find_or_create_root(&mut dom, Realm::Server);
    assert_eq!(dom.get(server_packages).unwrap().class(), "ServerPackages");
    let dev_packages = find_or_create_root(&mut dom, Realm::Dev);
    assert_eq!(dom.get(dev_packages).unwrap().class(), "DevPackages");
}

#[test]
fn find_or_create_root_reuses_an_existing_root_rather_than_duplicating_it() {
    let mut dom = WeakDom::new();
    let first = find_or_create_root(&mut dom, Realm::Shared);
    let second = find_or_create_root(&mut dom, Realm::Shared);
    assert_eq!(first, second);
    assert_eq!(dom.root_refs().len(), 1);
}

#[test]
fn find_or_create_child_reuses_a_child_matched_by_name_not_class() {
    let mut dom = WeakDom::new();
    let root = dom.new_instance("Packages", "Packages", None);
    let index = find_or_create_child(&mut dom, root, "_Index");
    assert_eq!(dom.get(index).unwrap().class(), "Folder");
    let same_index = find_or_create_child(&mut dom, root, "_Index");
    assert_eq!(index, same_index);
    assert_eq!(dom.get(root).unwrap().children().len(), 1);
}

fn leaf(name: &str, source: &str) -> PackageNode {
    PackageNode {
        name: name.to_owned(),
        source: Some(source.to_owned()),
        children: Vec::new(),
    }
}

#[test]
fn realize_node_makes_a_module_script_for_a_node_with_source() {
    let mut dom = WeakDom::new();
    let root = dom.new_instance("Folder", "root", None);
    let referent = realize_node(&mut dom, &leaf("init", "return {}"), root);
    assert_eq!(dom.get(referent).unwrap().class(), "ModuleScript");
    assert_eq!(source::read(&dom, referent).as_deref(), Some("return {}"));
}

#[test]
fn realize_node_makes_a_folder_for_a_node_with_no_source_and_recurses_into_children() {
    let mut dom = WeakDom::new();
    let root = dom.new_instance("Folder", "root", None);
    let tree = PackageNode {
        name: "pkg".to_owned(),
        source: None,
        children: vec![leaf("a", "return 1"), leaf("b", "return 2")],
    };
    let referent = realize_node(&mut dom, &tree, root);
    assert_eq!(dom.get(referent).unwrap().class(), "Folder");
    let children = dom.get(referent).unwrap().children().to_vec();
    assert_eq!(children.len(), 2);
    let names: Vec<_> = children
        .iter()
        .map(|&c| dom.get(c).unwrap().name().to_owned())
        .collect();
    assert!(names.contains(&"a".to_owned()));
    assert!(names.contains(&"b".to_owned()));
}
