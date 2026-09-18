use std::path::PathBuf;

use rbx_dom::Instance;

use crate::folder_colors::FolderColors;

use super::*;

fn node(class: &str, name: &str) -> Node {
    Node {
        id: 0,
        class: class.to_string(),
        name: name.to_string(),
        path: name.to_string(),
        children: Vec::new(),
    }
}

/// No place ever tags a folder in these tests — a fixed empty store and an
/// arbitrary place path, since nothing here exercises tag lookup itself
/// (that's `explorer_folder_tint_tests` below).
fn no_tags() -> (FolderColors, PathBuf) {
    (FolderColors::default(), PathBuf::from("/tmp/test.rbxl"))
}

fn from_dom(dom: &WeakDom) -> Explorer {
    let (colors, place) = no_tags();
    Explorer::from_dom(dom, &colors, &place)
}

fn names(nodes: &[Node]) -> Vec<&str> {
    nodes.iter().map(|node| node.name.as_str()).collect()
}

fn insert(dom: &mut WeakDom, id: u32, class: &str, name: &str) -> Ref {
    let reference = Ref::new(id);
    dom.insert(Instance::new(reference, class, name));
    reference
}

/// Whether a `ClassIcon` is the Lucide fallback, for assertions where the
/// exact icon does not matter, only that no sprite was substituted.
fn is_lucide(icon: &ClassIcon) -> bool {
    matches!(icon, ClassIcon::Lucide(_))
}

#[test]
fn services_come_in_studio_order() {
    let mut roots = vec![
        node("Lighting", "Lighting"),
        node("TextChatService", "TextChatService"),
        node("Workspace", "Workspace"),
        node("Players", "Players"),
    ];
    sort_roots(&mut roots);

    assert_eq!(
        names(&roots),
        ["Workspace", "Players", "Lighting", "TextChatService"]
    );
}

#[test]
fn unknown_roots_follow_the_services_alphabetically() {
    let mut roots = vec![
        node("Zebra", "Zebra"),
        node("Lighting", "Lighting"),
        node("Chat", "Chat"),
        node("Workspace", "Workspace"),
    ];
    sort_roots(&mut roots);

    assert_eq!(names(&roots), ["Workspace", "Lighting", "Chat", "Zebra"]);
}

#[test]
fn alphabetical_ordering_ignores_case() {
    let mut roots = vec![
        node("Chat", "Chat"),
        node("CSGDictionaryService", "CSGDictionaryService"),
        node("AvatarSettings", "AvatarSettings"),
    ];
    sort_roots(&mut roots);

    assert_eq!(
        names(&roots),
        ["AvatarSettings", "Chat", "CSGDictionaryService"]
    );
}

#[test]
fn a_missing_service_leaves_no_gap() {
    let mut roots = vec![node("Teams", "Teams"), node("Workspace", "Workspace")];
    sort_roots(&mut roots);

    assert_eq!(names(&roots), ["Workspace", "Teams"]);
}

#[test]
fn instances_keep_their_children_in_file_order() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", "Workspace");
    let camera = insert(&mut dom, 2, "Camera", "Camera");
    let baseplate = insert(&mut dom, 3, "Part", "Baseplate");
    dom.set_parent(camera, Some(workspace));
    dom.set_parent(baseplate, Some(workspace));

    let roots = roots(&dom);

    assert_eq!(names(&roots), ["Workspace"]);
    assert_eq!(names(&roots[0].children), ["Camera", "Baseplate"]);
}

#[test]
fn every_instance_gets_an_icon_by_its_referent() {
    let mut dom = WeakDom::new();
    // Part: covered by the icon kit. BodyColors: a real class the kit doesn't
    // claim a tile for, which is what this test is about (one icon per
    // referent, covered or not).
    let part = insert(&mut dom, 7, "Part", "Part");
    let colors = insert(&mut dom, 8, "BodyColors", "BodyColors");
    dom.set_parent(colors, Some(part));

    let explorer = from_dom(&dom);

    assert_eq!(explorer.items(true).len(), 1);
    assert!(matches!(explorer.icon(&"7".into()), ClassIcon::Sprite(_)));
    assert!(matches!(
        explorer.icon(&"8".into()),
        ClassIcon::Lucide(IconName::CircleDot)
    ));
}

#[test]
fn icons_follow_the_class_family() {
    assert_eq!(icon("Workspace"), IconName::Globe);
    assert_eq!(icon("MeshPart"), IconName::Box);
    assert_eq!(icon("LocalScript"), IconName::FileCode);
    assert_eq!(icon("ServerStorage"), IconName::Server);
    assert_eq!(icon("SoundService"), IconName::Server);
}

#[test]
fn an_unknown_class_falls_back_to_the_default_icon() {
    assert_eq!(icon("SurfaceAppearance"), IconName::CircleDot);
    assert_eq!(icon(""), IconName::CircleDot);
}

#[test]
fn resolve_icon_prefers_the_icon_kit_over_the_lucide_fallback() {
    assert!(matches!(resolve_icon("Script"), ClassIcon::Sprite(_)));
    assert!(is_lucide(&resolve_icon("BodyColors")));
}

#[test]
fn a_fixed_order_service_is_always_shown_by_default() {
    for service in SERVICE_ORDER {
        assert!(is_default_visible(service), "{service}");
    }
}

#[test]
fn a_deliberately_noisy_service_is_hidden_by_default() {
    for noisy in ["HttpService", "TestService", "Chat", "InsertService"] {
        assert!(!is_default_visible(noisy), "{noisy}");
    }
}

#[test]
fn an_instance_outside_the_known_service_set_is_never_hidden() {
    for real in ["Baseplate", "MyFolder", "SomeThirdPartyPlugin"] {
        assert!(is_default_visible(real), "{real}");
    }
}

#[test]
fn the_default_view_drops_noisy_services_the_full_view_keeps() {
    let mut dom = WeakDom::new();
    insert(&mut dom, 1, "Workspace", "Workspace");
    insert(&mut dom, 2, "HttpService", "HttpService");
    insert(&mut dom, 3, "MyFolder", "MyFolder");

    let explorer = from_dom(&dom);

    let labels = |show_all| -> Vec<String> {
        explorer
            .items(show_all)
            .iter()
            .map(|item| item.label.to_string())
            .collect()
    };
    assert_eq!(labels(false), ["Workspace", "MyFolder"]);
    assert_eq!(labels(true), ["Workspace", "HttpService", "MyFolder"]);
}

#[test]
fn a_row_id_round_trips_through_its_referent() {
    let reference = Ref::new(3);

    assert_eq!(item_id(reference), SharedString::from("3"));
    assert_eq!(item_ref(&item_id(reference)), Some(reference));
    assert_eq!(item_ref(&"Baseplate".into()), None);
}

#[test]
fn a_nested_instance_is_found_by_referent_even_under_a_hidden_root() {
    let mut dom = WeakDom::new();
    let http = insert(&mut dom, 1, "HttpService", "HttpService");
    let child = insert(&mut dom, 2, "Folder", "Hidden");
    let workspace = insert(&mut dom, 3, "Workspace", "Workspace");
    let baseplate = insert(&mut dom, 4, "Part", "Baseplate");
    dom.set_parent(child, Some(http));
    dom.set_parent(baseplate, Some(workspace));

    let explorer = from_dom(&dom);

    assert_eq!(
        explorer.item(baseplate).map(|item| item.label),
        Some("Baseplate".into())
    );
    assert_eq!(
        explorer.item(child).map(|item| item.label),
        Some("Hidden".into())
    );
    assert!(explorer.item(Ref::new(99)).is_none());
}

#[test]
fn a_tagged_folders_icon_is_recolored_to_its_tag() {
    let mut dom = WeakDom::new();
    let folder = insert(&mut dom, 1, "Folder", "Tagged");

    let mut colors = FolderColors::default();
    let place = PathBuf::from("/tmp/tagged.rbxl");
    colors.set(&place, "Tagged", (200, 60, 60));

    let untagged = from_dom(&dom);
    let tagged = Explorer::from_dom(&dom, &colors, &place);

    let (ClassIcon::Sprite(untagged_icon), ClassIcon::Sprite(tagged_icon)) = (
        untagged.icon(&item_id(folder)),
        tagged.icon(&item_id(folder)),
    ) else {
        panic!("Folder is always covered by the icon kit");
    };

    assert_ne!(untagged_icon.as_bytes(0), tagged_icon.as_bytes(0));
}

#[test]
fn an_untagged_folder_under_a_place_with_other_tags_keeps_its_plain_icon() {
    let mut dom = WeakDom::new();
    let plain = insert(&mut dom, 1, "Folder", "Plain");
    insert(&mut dom, 2, "Folder", "Elsewhere");

    let mut colors = FolderColors::default();
    let place = PathBuf::from("/tmp/tagged.rbxl");
    colors.set(&place, "Elsewhere", (200, 60, 60));

    let plain_explorer = from_dom(&dom);
    let mixed_explorer = Explorer::from_dom(&dom, &colors, &place);

    let (ClassIcon::Sprite(plain_bytes), ClassIcon::Sprite(mixed_bytes)) = (
        plain_explorer.icon(&item_id(plain)),
        mixed_explorer.icon(&item_id(plain)),
    ) else {
        panic!("Folder is always covered by the icon kit");
    };

    assert_eq!(plain_bytes.as_bytes(0), mixed_bytes.as_bytes(0));
}

#[test]
fn find_by_name_walks_depth_first_in_file_order() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", "Workspace");
    let model = insert(&mut dom, 2, "Model", "Model");
    let inner = insert(&mut dom, 3, "Part", "Part");
    let outer = insert(&mut dom, 4, "Part", "Part");
    dom.set_parent(model, Some(workspace));
    dom.set_parent(inner, Some(model));
    dom.set_parent(outer, Some(workspace));

    assert_eq!(find_by_name(&dom, "Workspace"), Some(workspace));
    // The one inside the model comes first: it is reached before the sibling
    // that follows the model in the file.
    assert_eq!(find_by_name(&dom, "Part"), Some(inner));
    assert_eq!(find_by_name(&dom, "Nowhere"), None);
}
