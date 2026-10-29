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

/// No place ever tags a folder in these tests, a fixed empty store and an
/// arbitrary place path, since nothing here exercises tag lookup itself
/// (that's `explorer_folder_tint_tests` below).
fn no_tags() -> (FolderColors, PathBuf) {
    (FolderColors::default(), PathBuf::from("/tmp/test.rbxl"))
}

fn from_dom(dom: &WeakDom) -> Explorer {
    let (colors, place) = no_tags();
    Explorer::from_dom(dom, IconPack::Dark, &colors, &place)
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
        node("CollectionService", "CollectionService"),
        node("CSGDictionaryService", "CSGDictionaryService"),
        node("AvatarSettings", "AvatarSettings"),
    ];
    sort_roots(&mut roots);

    assert_eq!(
        names(&roots),
        [
            "AvatarSettings",
            "CollectionService",
            "CSGDictionaryService"
        ]
    );
}

#[test]
fn a_service_ticked_to_show_takes_its_studio_place_not_the_end() {
    let mut roots = vec![
        node("AvatarSettings", "AvatarSettings"),
        node("TestService", "TestService"),
        node("TextChatService", "TextChatService"),
        node("Chat", "Chat"),
        node("SoundService", "SoundService"),
        node("Workspace", "Workspace"),
    ];
    sort_roots(&mut roots);

    assert_eq!(
        names(&roots),
        [
            "Workspace",
            "SoundService",
            "Chat",
            "TextChatService",
            "TestService",
            "AvatarSettings"
        ]
    );
}

#[test]
fn a_missing_service_leaves_no_gap() {
    let mut roots = vec![node("Teams", "Teams"), node("Workspace", "Workspace")];
    sort_roots(&mut roots);

    assert_eq!(names(&roots), ["Workspace", "Teams"]);
}

#[test]
fn children_sort_like_studios_explorer() {
    // Inserted shuffled; the expected order is what real Studio's Explorer
    // listed for the same Workspace (Studio 0.744). `k1`, a class with no
    // `ExplorerOrder`, is from a second place, where it listed below a
    // `Configuration`.
    let shuffled = [
        ("Part", "Zed"),
        ("Model", "beta"),
        ("Part", "Alpha"),
        ("Folder", "Mid"),
        ("Script", "aScript"),
        ("Part", "alpha2"),
        ("Model", "Alpha"),
        ("WedgePart", "W"),
        ("Camera", "Camera"),
        ("SpawnLocation", "SpawnLocation"),
        ("Part", "_under"),
        ("Part", "10"),
        ("Part", "9"),
        ("LocalScript", "Client"),
        ("Configuration", "Cfg"),
        ("Attachment", "Att"),
        ("Sound", "Snd"),
        ("MeshPart", "Mesh"),
        ("Folder", "apple"),
        ("KeyframeSequence", "k1"),
        ("Terrain", "Terrain"),
    ];
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", "Workspace");
    for (id, (class, name)) in (2..).zip(shuffled) {
        let child = insert(&mut dom, id, class, name);
        dom.set_parent(child, Some(workspace));
    }

    let roots = roots(&dom);

    assert_eq!(
        names(&roots[0].children),
        [
            "Camera",
            "Terrain",
            "Mid",
            "Snd",
            "apple",
            "Att",
            "SpawnLocation",
            "aScript",
            "Client",
            "Alpha",
            "beta",
            "Mesh",
            "10",
            "9",
            "Alpha",
            "Zed",
            "_under",
            "alpha2",
            "W",
            "Cfg",
            "k1",
        ]
    );
}

#[test]
fn every_instance_gets_an_icon_by_its_referent() {
    let mut dom = WeakDom::new();
    // Part: covered by the icon kit. PluginAction: a real class the kit
    // doesn't claim a tile for, which is what this test is about (one icon
    // per referent, covered or not).
    let part = insert(&mut dom, 7, "Part", "Part");
    let action = insert(&mut dom, 8, "PluginAction", "PluginAction");
    dom.set_parent(action, Some(part));

    let explorer = from_dom(&dom);

    assert_eq!(explorer.items(true, &ServiceOverrides::new()).len(), 1);
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
    assert!(matches!(
        resolve_icon("Script", IconPack::Dark),
        ClassIcon::Sprite(_)
    ));
    assert!(is_lucide(&resolve_icon("PluginAction", IconPack::Dark)));
}

#[test]
fn set_icon_pack_swaps_every_row_s_sprite_without_touching_its_tree_item() {
    let mut dom = WeakDom::new();
    insert(&mut dom, 1, "Part", "Baseplate");

    let (colors, place) = no_tags();
    let dark = Explorer::from_dom(&dom, IconPack::Dark, &colors, &place);
    let dark_bytes = match dark.icon(&"1".into()) {
        ClassIcon::Sprite(image) => image.as_bytes(0).map(<[u8]>::to_vec),
        ClassIcon::Lucide(_) => None,
    };

    let light = dark.set_icon_pack(IconPack::Light, &colors, &place);
    let light_bytes = match light.icon(&"1".into()) {
        ClassIcon::Sprite(image) => image.as_bytes(0).map(<[u8]>::to_vec),
        ClassIcon::Lucide(_) => None,
    };

    assert!(dark_bytes.is_some() && light_bytes.is_some());
    assert_ne!(dark_bytes, light_bytes);
    // The row itself (id, label, children) is unaffected by the pack swap.
    assert_eq!(
        dark.items(true, &ServiceOverrides::new())[0].label,
        light.items(true, &ServiceOverrides::new())[0].label
    );
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
fn a_service_newer_than_the_list_is_hidden_by_its_dump_tag() {
    for newer in ["AvatarSettings", "ConfigureServerService"] {
        assert!(is_known_service(newer), "{newer}");
        assert!(!is_default_visible(newer), "{newer}");
    }
}

#[test]
fn avatar_settings_stays_out_of_even_the_full_view() {
    let mut dom = WeakDom::new();
    insert(&mut dom, 1, "Workspace", "Workspace");
    insert(&mut dom, 2, "AvatarSettings", "AvatarSettings");
    insert(&mut dom, 3, "HttpService", "HttpService");

    let explorer = from_dom(&dom);
    let mut shown = ServiceOverrides::new();
    shown.insert("AvatarSettings".into(), true);

    let labels: Vec<String> = explorer
        .items(true, &shown)
        .iter()
        .map(|item| item.label.to_string())
        .collect();
    assert_eq!(labels, ["Workspace", "HttpService"]);
    assert_eq!(explorer.items(false, &shown).len(), 1);
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
            .items(show_all, &ServiceOverrides::new())
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
    let tagged = Explorer::from_dom(&dom, IconPack::Dark, &colors, &place);

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
    let mixed_explorer = Explorer::from_dom(&dom, IconPack::Dark, &colors, &place);

    let (ClassIcon::Sprite(plain_bytes), ClassIcon::Sprite(mixed_bytes)) = (
        plain_explorer.icon(&item_id(plain)),
        mixed_explorer.icon(&item_id(plain)),
    ) else {
        panic!("Folder is always covered by the icon kit");
    };

    assert_eq!(plain_bytes.as_bytes(0), mixed_bytes.as_bytes(0));
}

// The row tints the tree paints with, worked out with the rows rather than
// by walking the DOM every render: only tagged folders, keyed by row id, and
// kept by an icon-pack switch.
#[test]
fn only_tagged_folders_have_a_row_tint() {
    let mut dom = WeakDom::new();
    let tagged = insert(&mut dom, 1, "Folder", "Tagged");
    insert(&mut dom, 2, "Folder", "Plain");
    insert(&mut dom, 3, "Model", "Tagged2");

    let mut colors = FolderColors::default();
    let place = PathBuf::from("/tmp/tagged.rbxl");
    colors.set(&place, "Tagged", (200, 60, 60));
    colors.set(&place, "Tagged2", (1, 2, 3));

    let explorer = Explorer::from_dom(&dom, IconPack::Dark, &colors, &place);
    let expected = HashMap::from([(item_id(tagged), (200, 60, 60))]);
    assert_eq!(explorer.folder_tints(), &expected);

    let switched = explorer.set_icon_pack(IconPack::Light, &colors, &place);
    assert_eq!(switched.folder_tints(), &expected);
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

/// The tree `resolve` is exercised against below: two `Part`s called `Part`,
/// one nested under a `Model` and one directly under `Workspace`, so a path
/// and a name genuinely disagree about which one is meant.
fn ambiguous_place() -> (WeakDom, Ref, Ref) {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", "Workspace");
    let model = insert(&mut dom, 2, "Model", "Model");
    let inner = insert(&mut dom, 3, "Part", "Part");
    let outer = insert(&mut dom, 4, "Part", "Part");
    dom.set_parent(model, Some(workspace));
    dom.set_parent(inner, Some(model));
    dom.set_parent(outer, Some(workspace));
    (dom, inner, outer)
}

#[test]
fn a_path_picks_the_instance_a_bare_name_would_miss() {
    let (dom, inner, outer) = ambiguous_place();

    // The name search reaches the nested one first (see
    // `find_by_name_walks_depth_first_in_file_order`), so the sibling under
    // Workspace is only reachable by path.
    assert_eq!(resolve(&dom, "Part"), Some(inner));
    assert_eq!(resolve(&dom, "Workspace.Part"), Some(outer));
    assert_eq!(resolve(&dom, "Workspace.Model.Part"), Some(inner));
}

#[test]
fn a_path_matches_children_only_never_deeper_descendants() {
    let (dom, _, _) = ambiguous_place();

    // `Part` is a grandchild of Workspace through `Model`, not a child of it,
    // so this path has no match at all, and no bare name to fall back to.
    assert_eq!(resolve(&dom, "Workspace.Model.Model.Part"), None);
    assert_eq!(resolve(&dom, "Model.Part"), None);
}

#[test]
fn a_name_containing_a_dot_still_resolves_once_the_path_fails() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", "Workspace");
    let dotted = insert(&mut dom, 2, "Folder", "Config.v2");
    dom.set_parent(dotted, Some(workspace));

    assert_eq!(resolve(&dom, "Config.v2"), Some(dotted));
    assert_eq!(resolve(&dom, "Workspace.Config.v2"), Some(dotted));
}

#[test]
fn the_longest_child_name_wins_inside_a_path() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", "Workspace");
    let spec = insert(&mut dom, 2, "ModuleScript", "Smoke.spec");
    let smoke = insert(&mut dom, 3, "Folder", "Smoke");
    let nested = insert(&mut dom, 4, "ModuleScript", "spec");
    let dotted_folder = insert(&mut dom, 5, "Folder", "Tests.unit");
    let inside = insert(&mut dom, 6, "ModuleScript", "Smoke.spec");
    dom.set_parent(spec, Some(workspace));
    dom.set_parent(smoke, Some(workspace));
    dom.set_parent(nested, Some(smoke));
    dom.set_parent(dotted_folder, Some(workspace));
    dom.set_parent(inside, Some(dotted_folder));

    // `Smoke.spec` the script beats `Smoke` the folder's child `spec`.
    assert_eq!(resolve(&dom, "Workspace.Smoke.spec"), Some(spec));
    assert_eq!(
        resolve(&dom, "Workspace.Tests.unit.Smoke.spec"),
        Some(inside)
    );
    assert_eq!(resolve(&dom, "Smoke.spec"), Some(spec));
}

#[test]
fn a_shorter_name_is_tried_when_the_longest_dead_ends() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", "Workspace");
    let dotted = insert(&mut dom, 2, "Folder", "A.B");
    let plain = insert(&mut dom, 3, "Folder", "A");
    let leaf = insert(&mut dom, 4, "Part", "B.C");
    dom.set_parent(dotted, Some(workspace));
    dom.set_parent(plain, Some(workspace));
    dom.set_parent(leaf, Some(plain));

    // `A.B` matches first but has no `C` under it; `A` then `B.C` does.
    assert_eq!(resolve(&dom, "Workspace.A.B.C"), Some(leaf));
}

#[test]
fn a_root_resolves_by_the_single_segment_that_names_it() {
    let (dom, _, _) = ambiguous_place();

    assert_eq!(resolve(&dom, "Workspace"), Some(Ref::new(1)));
    assert_eq!(resolve(&dom, "Nowhere"), None);
    assert_eq!(resolve(&dom, ""), None);
}

#[test]
fn the_tiles_authored_beyond_roblox_own_sheet_resolve_to_sprites() {
    // The five classes the insert picker's list is the reason for: real
    // Studio's own metadata gives them no tile, so before the kit grew one
    // each they listed with a bare Lucide glyph.
    for class in [
        "StyleSheet",
        "StyleRule",
        "StyleLink",
        "IntersectOperation",
        "BodyColors",
    ] {
        assert!(
            matches!(resolve_icon(class, IconPack::Dark), ClassIcon::Sprite(_)),
            "{class} should draw a kit tile"
        );
        assert!(
            matches!(resolve_icon(class, IconPack::Light), ClassIcon::Sprite(_)),
            "{class} should draw a kit tile in the light variant too"
        );
    }
}

#[test]
fn the_picker_and_the_tree_resolve_one_class_to_one_tile() {
    // `shell::explorer_edit::picker` lists classes and the tree lists
    // instances, but both go through `resolve_icon`: so the same class has
    // to come back as the very same rasterized tile, not a second copy of
    // an equal one.
    let mut dom = WeakDom::new();
    insert(&mut dom, 11, "Part", "Part");
    let explorer = from_dom(&dom);

    match (
        explorer.icon(&"11".into()),
        resolve_icon("Part", IconPack::Dark),
    ) {
        (ClassIcon::Sprite(tree), ClassIcon::Sprite(picker)) => {
            assert!(Arc::ptr_eq(&tree, &picker));
        }
        _ => panic!("Part is covered by the icon kit"),
    }
}

// The UI editor's Explorer: the UI roots wherever they sit, under
// `StarterGui` or on a part, each with its whole subtree, and not one row
// of the rest of the place.
#[test]
fn the_ui_view_lists_only_ui_roots_and_what_is_under_them() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", "Workspace");
    let part = insert(&mut dom, 2, "Part", "Sign");
    dom.set_parent(part, Some(workspace));
    let surface = insert(&mut dom, 3, "SurfaceGui", "Face");
    dom.set_parent(surface, Some(part));
    let starter = insert(&mut dom, 4, "StarterGui", "StarterGui");
    let hud = insert(&mut dom, 5, "ScreenGui", "Hud");
    dom.set_parent(hud, Some(starter));
    let frame = insert(&mut dom, 6, "Frame", "Bar");
    dom.set_parent(frame, Some(hud));
    insert(&mut dom, 7, "Lighting", "Lighting");

    let explorer = from_dom(&dom);
    let roots = explorer.ui_items();
    let ids: Vec<&str> = roots.iter().map(|item| item.id.as_ref()).collect();
    assert_eq!(
        ids,
        ["3", "5"],
        "services first, so Workspace's surface leads"
    );
    assert_eq!(roots[1].children.len(), 1);
    assert_eq!(roots[1].children[0].id.as_ref(), "6");
}

fn labels(items: &[TreeItem]) -> Vec<String> {
    items.iter().map(|item| item.label.to_string()).collect()
}

/// A search keeps the matches and the path to them, opens that path, and
/// leaves each match's own children whole so it can be browsed in place.
#[test]
fn a_search_keeps_matches_under_their_path_with_their_children_whole() {
    let door = TreeItem::new("door", "Door").child(TreeItem::new("weld", "Weld"));
    let model = TreeItem::new("model", "House")
        .child(door)
        .child(TreeItem::new("roof", "Roof"));
    let items = vec![
        TreeItem::new("ws", "Workspace").child(model),
        TreeItem::new("light", "Lighting"),
    ];

    let found = search(&items, "  DOOR ");
    assert_eq!(labels(&found), ["Workspace"]);
    let house = &found[0].children[0];
    assert_eq!(labels(&house.children), ["Door"], "Roof does not match");
    assert_eq!(
        labels(&house.children[0].children),
        ["Weld"],
        "a match keeps its children"
    );
    assert!(
        found[0].is_expanded() && house.is_expanded(),
        "the path is opened"
    );
    // The path is opened in the results alone; a match shares its row's
    // state, so opening it here opens it in the unfiltered tree too.
    assert!(!items[0].is_expanded() && !items[0].children[0].is_expanded());
    let _ = house.children[0].clone().expanded(true);
    assert!(items[0].children[0].children[0].is_expanded());

    assert_eq!(labels(&search(&items, "light")), ["Lighting"]);
    assert!(search(&items, "nothing like it").is_empty());
    assert_eq!(labels(&search(&items, "   ")), ["Workspace", "Lighting"]);
    // Case is ignored beyond ASCII too.
    let accented = [TreeItem::new("e", "Écran")];
    assert_eq!(labels(&search(&accented, "éCRAN")), ["Écran"]);
}

/// A service the user ticks joins the default view, one they untick leaves
/// it, and flipping either back removes the override rather than storing
/// Studio's own answer.
#[test]
fn default_services_follow_the_users_overrides_and_store_only_differences() {
    let mut dom = WeakDom::new();
    insert(&mut dom, 1, "Workspace", "Workspace");
    insert(&mut dom, 2, "HttpService", "HttpService");
    insert(&mut dom, 3, "Teams", "Teams");
    let explorer = from_dom(&dom);
    let labels = |overrides: &ServiceOverrides| -> Vec<String> {
        explorer
            .items(false, overrides)
            .iter()
            .map(|item| item.label.to_string())
            .collect()
    };
    assert_eq!(labels(&ServiceOverrides::new()), ["Workspace", "Teams"]);

    let overrides = toggled(ServiceOverrides::new(), "HttpService");
    let overrides = toggled(overrides, "Teams");
    assert_eq!(labels(&overrides), ["Workspace", "HttpService"]);
    assert_eq!(overrides.get("HttpService"), Some(&true));
    assert_eq!(overrides.get("Teams"), Some(&false));

    let overrides = toggled(toggled(overrides, "HttpService"), "Teams");
    assert!(overrides.is_empty());
    assert_eq!(labels(&overrides), ["Workspace", "Teams"]);
}

/// An override only reaches a service: a hand-edited `{"Folder": false}`
/// must not hide the place's own root folders.
#[test]
fn an_override_for_a_non_service_class_is_ignored() {
    let overrides = ServiceOverrides::from([("Folder".to_owned(), false)]);
    assert!(is_listed("Folder", &overrides));
}
