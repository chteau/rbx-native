use rbx_dom::{Change, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::choices::{rank, Tier};
use super::*;
use crate::history::{History, DEFAULT_CAP};

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

fn vector3(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

/// The one default these tests need, spelled the way the DOM stores it.
fn stock_size(size: Variant) -> Vec<(&'static str, Variant)> {
    vec![("size", size)]
}

/// A `Part` under `Workspace`, holding what a real one from a file holds:
/// serialized spellings (`size`, `shape`, `Color3uint8`), a tag and an
/// attribute blob.
fn place() -> (WeakDom, Ref, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Wall", Some(workspace));
    for (key, value) in [
        ("size", vector3(10.0, 4.0, 1.0)),
        ("shape", Variant::Enum(1)),
        ("Color3uint8", Variant::Color3uint8 { r: 1, g: 2, b: 3 }),
        ("Transparency", Variant::Float32(0.25)),
        ("Tags", Variant::String("Door\0".into())),
        ("AttributesSerialize", Variant::String("\u{1}\0\0\0".into())),
    ] {
        dom.set_property(part, key, value).unwrap();
    }
    dom.take_changes();
    (dom, workspace, part)
}

fn plan_for(dom: &WeakDom, part: Ref, target: &str) -> Plan {
    plan(&database(), dom.get(part).unwrap(), target, &[])
}

#[test]
fn a_property_the_new_class_lacks_is_dropped_by_its_reflected_name() {
    let (dom, _, part) = place();
    let plan = plan_for(&dom, part, "MeshPart");
    // `shape` is `Part.Shape`; a `MeshPart` has no such thing.
    assert_eq!(plan.dropped, vec!["Shape"]);
    for key in ["size", "Color3uint8", "Transparency"] {
        assert!(plan.kept.iter().any(|kept| kept == key), "{key} is kept");
    }
}

// Read raw, `size` and `Color3uint8` are names the dump has never heard of
// and would be kept as unknown; read through the mapping they are
// `BasePart.Size` and `BasePart.Color`, which a `Folder` does not have.
#[test]
fn serialized_spellings_are_compared_as_the_properties_they_stand_for() {
    let (dom, _, part) = place();
    let plan = plan_for(&dom, part, "Folder");
    let mut dropped = plan.dropped.clone();
    dropped.sort();
    assert_eq!(dropped, vec!["Color", "Shape", "Size", "Transparency"]);
}

#[test]
fn tags_and_attributes_survive_any_change() {
    let (dom, _, part) = place();
    for target in ["Folder", "Frame", "StringValue", "MeshPart"] {
        let plan = plan_for(&dom, part, target);
        for key in ["Tags", "AttributesSerialize"] {
            assert!(plan.kept.iter().any(|kept| kept == key), "{key} → {target}");
        }
    }
}

// `BasePart.Size` is a `Vector3`, `GuiObject.Size` a `UDim2`: the same name
// is not the same property.
#[test]
fn a_property_of_the_same_name_but_another_type_is_dropped() {
    let (dom, _, part) = place();
    let plan = plan_for(&dom, part, "Frame");
    assert!(plan.dropped.iter().any(|name| name == "Size"));
    assert!(plan.kept.iter().any(|kept| kept == "Transparency"));
}

// Read from the per-class table (`ReflectionDatabase::default_value`): a
// stock `Part` is 4 × 1.2 × 2, a stock `TrussPart` 2 × 2 × 2.
#[test]
fn a_value_at_the_old_default_gives_way_to_the_new_one() {
    let (mut dom, _, part) = place();
    dom.set_property(part, "size", vector3(4.0, 1.2, 2.0))
        .unwrap();

    let plan = plan_for(&dom, part, "TrussPart");
    apply(&mut dom, part, "TrussPart", &plan);

    assert_eq!(plan.reset, vec!["size"]);
    assert_eq!(
        dom.get(part).unwrap().properties().get("size"),
        Some(&vector3(2.0, 2.0, 2.0))
    );
}

// Not only parts: a stock `PointLight` reaches 8 studs, a stock `SpotLight`
// 16, and a light nobody tuned should light like the one it now is.
#[test]
fn a_light_at_its_stock_range_takes_the_new_class_range() {
    let mut dom = WeakDom::new();
    let light = dom.new_instance("PointLight", "Light", None);
    dom.set_property(light, "Range", Variant::Float32(8.0))
        .unwrap();
    dom.set_property(light, "Brightness", Variant::Float32(1.0))
        .unwrap();

    let plan = plan_for(&dom, light, "SpotLight");
    apply(&mut dom, light, "SpotLight", &plan);

    let properties = dom.get(light).unwrap().properties();
    assert_eq!(properties.get("Range"), Some(&Variant::Float32(16.0)));
    // Stock on both, so there is nothing to reset.
    assert_eq!(properties.get("Brightness"), Some(&Variant::Float32(1.0)));
    assert_eq!(plan.reset, vec!["Range"]);
}

// The table records `BasePart.Color` as a `Color3`; a file keeps it as a
// `Color3uint8`. Rewriting the key in the table's type would corrupt it.
#[test]
fn a_default_in_another_type_than_the_file_keeps_is_not_written() {
    let (mut dom, _, part) = place();
    let stock = Variant::Color3uint8 {
        r: 163,
        g: 162,
        b: 165,
    };
    dom.set_property(part, "Color3uint8", stock.clone())
        .unwrap();

    let plan = plan_for(&dom, part, "TrussPart");
    apply(&mut dom, part, "TrussPart", &plan);

    assert!(plan.kept.iter().any(|kept| kept == "Color3uint8"));
    assert_eq!(
        dom.get(part).unwrap().properties().get("Color3uint8"),
        Some(&stock)
    );
}

#[test]
fn a_value_someone_chose_is_kept_over_the_new_default() {
    let (mut dom, _, part) = place();
    let plan = plan_for(&dom, part, "TrussPart");
    apply(&mut dom, part, "TrussPart", &plan);
    assert_eq!(
        dom.get(part).unwrap().properties().get("size"),
        Some(&vector3(10.0, 4.0, 1.0))
    );
}

// A `Folder` becoming a `Part` has no size to keep, and a part with none is
// not one anything can draw.
#[test]
fn a_default_the_instance_never_had_is_filled_in() {
    let mut dom = WeakDom::new();
    let folder = dom.new_instance("Folder", "Folder", None);
    let stock = vector3(4.0, 1.2, 2.0);
    let plan = plan(
        &database(),
        dom.get(folder).unwrap(),
        "Part",
        &stock_size(stock.clone()),
    );
    apply(&mut dom, folder, "Part", &plan);
    assert_eq!(
        dom.get(folder).unwrap().properties().get("size"),
        Some(&stock)
    );
}

#[test]
fn applying_keeps_the_referent_and_logs_the_class_change() {
    let (mut dom, workspace, part) = place();
    let weld = dom.new_instance("Weld", "Weld", Some(workspace));
    dom.set_property(weld, "Part0", Variant::Ref(part)).unwrap();
    dom.take_changes();

    let plan = plan_for(&dom, part, "WedgePart");
    apply(&mut dom, part, "WedgePart", &plan);

    let instance = dom.get(part).unwrap();
    assert_eq!(instance.class(), "WedgePart");
    assert_eq!(instance.properties().get("shape"), None);
    assert_eq!(
        dom.get(weld).unwrap().properties().get("Part0"),
        Some(&Variant::Ref(part)),
        "what pointed at the part still points at it"
    );
    assert!(dom.take_changes().contains(&Change::Class(part)));
}

#[test]
fn undo_restores_the_old_class_and_every_dropped_property() {
    let (mut dom, _, part) = place();
    let before = dom.get(part).unwrap().clone();
    let mut history = History::new(DEFAULT_CAP);
    history.push(dom.clone());

    let plan = plan_for(&dom, part, "Folder");
    apply(&mut dom, part, "Folder", &plan);
    history.record_changes(dom.take_changes());

    let (previous, changes) = history.undo(dom.clone()).expect("the change to undo");
    assert_eq!(previous.get(part), Some(&before));
    assert!(changes.contains(&Change::Class(part)));

    let (next, _) = history.redo(previous).expect("the change to redo");
    assert_eq!(next.get(part).unwrap().class(), "Folder");
}

#[test]
fn a_service_and_a_service_root_cannot_change_class() {
    let (mut dom, workspace, part) = place();
    let packages = dom.new_instance("Packages", "Packages", None);
    let database = database();
    assert!(!changeable(&dom, &database, workspace));
    // Not in the dump at all, but the Explorer shows it as a service.
    assert!(!changeable(&dom, &database, packages));
    assert!(changeable(&dom, &database, part));
    assert!(!changeable(&dom, &database, Ref::new(999)));
}

#[test]
fn only_a_creatable_browsable_non_service_class_is_a_target() {
    let database = database();
    assert!(is_target(&database, "WedgePart"));
    assert!(!is_target(&database, "Terrain"), "NotCreatable");
    assert!(!is_target(&database, "Workspace"), "a service");
    assert!(!is_target(&database, "BasePart"), "abstract");
    assert!(!is_target(&database, "NoSuchClass"));
}

#[test]
fn a_mixed_selection_converts_what_it_can_and_skips_what_is_already_there() {
    let (mut dom, workspace, part) = place();
    let wedge = dom.new_instance("WedgePart", "Wedge", Some(workspace));
    let (convert, refused) = partition(
        &dom,
        &database(),
        &[part, workspace, wedge, Ref::new(999)],
        "WedgePart",
    );
    assert_eq!(convert, vec![part]);
    assert_eq!(refused, vec![workspace]);
}

#[test]
fn the_summary_names_what_is_lost() {
    let lossy = Plan {
        kept: vec!["a".into(), "b".into()],
        reset: vec!["c".into()],
        dropped: vec![
            "MeshId".into(),
            "TextureID".into(),
            "DoubleSided".into(),
            "X".into(),
        ],
        ..Plan::default()
    };
    let lossless = Plan {
        kept: vec!["a".into()],
        ..Plan::default()
    };
    assert_eq!(
        summary(std::slice::from_ref(&lossy)),
        "Keeps 3 properties · drops 4: DoubleSided, MeshId, TextureID, …"
    );
    assert_eq!(
        summary(std::slice::from_ref(&lossless)),
        "Keeps 1 property · drops none"
    );
    assert_eq!(
        summary(&[lossy.clone(), lossy]),
        "Drops 4 across 2: DoubleSided, MeshId, TextureID, …"
    );
    assert_eq!(summary(&[]), "");
}

fn suggested(sources: &[&str], recent: &[String]) -> Vec<String> {
    choices(&database(), sources, recent, "")
        .suggested
        .into_iter()
        .map(|choice| choice.class)
        .collect()
}

#[test]
fn a_part_is_offered_its_family_nearest_first() {
    let offered = suggested(&["Part"], &[]);
    for class in [
        "MeshPart",
        "WedgePart",
        "CornerWedgePart",
        "TrussPart",
        "Seat",
        "SpawnLocation",
    ] {
        assert!(offered.iter().any(|c| c == class), "{class} in {offered:?}");
    }
    let at = |class: &str| offered.iter().position(|c| c == class).unwrap();
    assert!(at("Seat") < at("WedgePart"), "its own subclasses first");
    assert!(at("WedgePart") < at("MeshPart"), "then FormFactorPart's");
    assert!(offered.len() <= 8);
    assert!(!offered.iter().any(|c| c == "Part"));
}

#[test]
fn scripts_guis_lights_and_values_are_offered_their_siblings() {
    assert_eq!(
        suggested(&["Script"], &[]),
        vec!["LocalScript", "ModuleScript"]
    );
    assert_eq!(
        suggested(&["PointLight"], &[]),
        vec!["SpotLight", "SurfaceLight"]
    );
    let frame = suggested(&["Frame"], &[]);
    for class in ["TextLabel", "TextButton", "ImageLabel", "ScrollingFrame"] {
        assert!(frame.iter().any(|c| c == class), "{class} in {frame:?}");
    }
    assert!(suggested(&["IntValue"], &[])
        .iter()
        .any(|c| c == "NumberValue"));
}

// Relatives stop short of `Instance`, where everything is related.
#[test]
fn a_class_with_no_near_family_is_offered_only_what_was_used_recently() {
    let recent = vec!["Model".to_owned(), "Configuration".to_owned()];
    assert_eq!(suggested(&["Folder"], &recent), recent);
}

#[test]
fn recent_classes_follow_the_relatives_without_repeating_them() {
    let recent = vec!["Folder".to_owned(), "WedgePart".to_owned()];
    let offered = suggested(&["Part"], &recent);
    assert_eq!(offered.last().map(String::as_str), Some("Folder"));
    assert_eq!(offered.iter().filter(|c| *c == "WedgePart").count(), 1);
}

#[test]
fn a_mixed_selection_is_offered_the_family_of_its_common_superclass() {
    let offered = suggested(&["Part", "MeshPart"], &[]);
    // Each is something the other half of the selection could become.
    assert!(offered.iter().any(|c| c == "Part"));
    assert!(offered.iter().any(|c| c == "MeshPart"));
}

#[test]
fn the_selection_s_own_class_is_greyed() {
    let listed = choices(&database(), &["Part"], &[], "part");
    let part = listed.iter().find(|c| c.class == "Part").unwrap();
    assert!(!part.legal);
    let wedge = listed.iter().find(|c| c.class == "WedgePart").unwrap();
    assert!(wedge.legal);
}

#[test]
fn matches_rank_prefix_then_word_starts_then_scattered() {
    assert_eq!(rank("spot", "SpotLight"), Some(Tier::Prefix));
    assert_eq!(rank("mp", "MeshPart"), Some(Tier::WordStart));
    assert_eq!(rank("tl", "TextLabel"), Some(Tier::WordStart));
    assert_eq!(rank("texlab", "TextLabel"), Some(Tier::WordStart));
    assert_eq!(rank("list", "UIListLayout"), Some(Tier::WordStart));
    assert_eq!(rank("xtl", "TextLabel"), Some(Tier::Scattered));
    assert_eq!(rank("zz", "TextLabel"), None);
}

fn first_match(query: &str) -> String {
    choices(&database(), &["Part"], &[], query).rest[0]
        .class
        .clone()
}

#[test]
fn the_best_match_comes_first() {
    assert_eq!(first_match("mp"), "MeshPart");
    assert_eq!(first_match("tl"), "TextLabel");
    assert_eq!(first_match("spot"), "SpotLight");
    // Shorter names win a tie: both start with "part".
    let listed = choices(&database(), &["Folder"], &[], "part");
    let at = |class: &str| listed.iter().position(|c| c.class == class).unwrap();
    assert!(at("Part") < at("PartOperation"));
}
