use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{candidate, field_label, RefPick};

/// A house model with a handle part inside it, and a `Weld` and an
/// `ObjectValue` beside it, all under `Workspace`.
struct Place {
    dom: WeakDom,
    house: Ref,
    handle: Ref,
    weld: Ref,
    value: Ref,
}

fn place() -> Place {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let house = dom.new_instance("Model", "House", Some(workspace));
    let handle = dom.new_instance("Part", "Handle", Some(house));
    let weld = dom.new_instance("Weld", "Weld", Some(workspace));
    let value = dom.new_instance("ObjectValue", "Value", Some(workspace));
    Place {
        dom,
        house,
        handle,
        weld,
        value,
    }
}

/// A viewport click on `hits` while `owner`'s `name` is armed.
fn pick(place: &Place, owner: Ref, name: &str, hits: &[Ref], alt: bool) -> Option<Ref> {
    candidate(&place.dom, &db(), hits, &[owner], name, alt)
}

fn db() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn a_plain_click_picks_what_a_click_would_select() {
    let place = place();
    // `ObjectValue.Value` holds any instance, so the house a plain click
    // selects is what it gets — and `Alt` reaches the part, as it does
    // for a selection.
    let hits = [place.handle];
    assert_eq!(
        pick(&place, place.value, "Value", &hits, false),
        Some(place.house)
    );
    assert_eq!(
        pick(&place, place.value, "Value", &hits, true),
        Some(place.handle)
    );
}

#[test]
fn a_part_typed_ref_takes_the_part_under_the_cursor_over_its_model() {
    let place = place();
    let hits = [place.handle];
    assert_eq!(
        pick(&place, place.weld, "Part0", &hits, false),
        Some(place.handle)
    );
    assert_eq!(
        pick(&place, place.house, "PrimaryPart", &hits, false),
        Some(place.handle)
    );
}

#[test]
fn every_selected_owner_has_to_take_the_part() {
    let mut place = place();
    let workspace = place.dom.root_refs()[0];
    let second = place.dom.new_instance("Weld", "Weld2", Some(workspace));
    let other = place.dom.new_instance("Model", "Truck", Some(workspace));
    let hits = [place.handle];
    // Two welds both take the part.
    assert_eq!(
        candidate(
            &place.dom,
            &db(),
            &hits,
            &[place.weld, second],
            "Part0",
            false
        ),
        Some(place.handle)
    );
    // The house would take its own handle as `PrimaryPart`, the truck
    // would not: the pick stays on the house, which both then refuse,
    // rather than a part written to one model and refused by the other.
    assert_eq!(
        candidate(
            &place.dom,
            &db(),
            &hits,
            &[place.house, other],
            "PrimaryPart",
            false
        ),
        Some(place.house)
    );
}

#[test]
fn an_empty_field_names_what_it_waits_for() {
    // Hovered and empty: what Studio's `SelectInstanceType` hint says.
    assert_eq!(
        field_label("nil", true, false, true, Some("BasePart")),
        "Select BasePart…"
    );
    // Picking outranks hovering.
    assert_eq!(
        field_label("nil", true, true, true, Some("BasePart")),
        "Selecting…"
    );
    assert_eq!(
        field_label("nil", true, false, false, Some("BasePart")),
        "nil"
    );
    // A value is always shown, picking or hovered.
    assert_eq!(
        field_label("Wheel", false, true, true, Some("BasePart")),
        "Wheel"
    );
    assert_eq!(
        field_label("Wheel", false, false, true, Some("BasePart")),
        "Wheel"
    );
}

#[test]
fn the_sky_picks_nothing() {
    let place = place();
    assert_eq!(pick(&place, place.weld, "Part0", &[], false), None);
}

#[test]
fn a_second_click_disarms_and_another_row_takes_the_pick() {
    let mut pick = RefPick::default();
    pick.toggle("Part0", false);
    assert!(pick.is_armed());
    pick.toggle("Part0", false);
    assert!(!pick.is_armed());

    pick.toggle("Part0", false);
    pick.toggle("Part1", false);
    assert_eq!(pick.take().as_deref(), Some("Part1"));
    // Taken once: Escape or a second Explorer press finds nothing armed.
    assert_eq!(pick.take(), None);
}

#[test]
fn arming_clears_the_last_refusal() {
    let mut pick = RefPick {
        error: Some(("Part1".to_owned(), "refused".to_owned())),
        ..RefPick::default()
    };
    assert_eq!(pick.error_for("Part1"), Some("refused"));
    assert_eq!(pick.error_for("Part0"), None);
    pick.toggle("Part1", false);
    assert_eq!(pick.error_for("Part1"), None);
}

#[test]
fn a_content_row_remembers_its_pick_names_an_object() {
    let mut pick = RefPick::default();
    pick.toggle("TextureContent", true);
    assert!(pick.is_armed_for("TextureContent") && pick.content);
    // Moving the pick to a `Ref` row commits a referent again.
    pick.toggle("Part0", false);
    assert!(!pick.is_armed_for("TextureContent") && !pick.content);
}
