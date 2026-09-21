use rbx_dom::{CFrameData, Color3Data, Instance, Vector3Data};

use super::*;

const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn insert(
    dom: &mut WeakDom,
    id: u32,
    class: &str,
    parent: Option<Ref>,
    properties: Vec<(&str, Variant)>,
) -> Ref {
    let referent = Ref::new(id);
    let mut instance = Instance::new(referent, class, class);
    for (name, value) in properties {
        instance.properties_mut().insert(name.to_owned(), value);
    }
    dom.insert(instance);
    dom.set_parent(referent, parent);
    referent
}

fn part(dom: &mut WeakDom, id: u32, parent: Ref) -> Ref {
    insert(
        dom,
        id,
        "Part",
        Some(parent),
        vec![
            (
                "size",
                Variant::Vector3(Vector3Data {
                    x: 4.0,
                    y: 1.0,
                    z: 2.0,
                }),
            ),
            (
                "CFrame",
                Variant::CFrame(CFrameData {
                    position: Vector3Data {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    rotation: IDENTITY_ROTATION,
                }),
            ),
        ],
    )
}

/// A Workspace holding a Model of two parts, plus a loose part beside it.
fn place() -> (WeakDom, Ref, Ref, [Ref; 2], Ref) {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", None, Vec::new());
    let model = insert(&mut dom, 2, "Model", Some(workspace), Vec::new());
    let first = part(&mut dom, 3, model);
    let second = part(&mut dom, 4, model);
    let loose = part(&mut dom, 5, workspace);
    (dom, workspace, model, [first, second], loose)
}

/// The covered parts are a set, not a sequence — see `Highlight::parts` —
/// so every assertion about them compares them as one.
fn sorted(parts: &[Ref]) -> Vec<Ref> {
    let mut sorted = parts.to_vec();
    sorted.sort();
    sorted
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn a_highlight_covers_every_part_under_the_model_it_hangs_off() {
    let (mut dom, _, model, parts, _) = place();
    insert(&mut dom, 9, "Highlight", Some(model), Vec::new());

    let planned = plan(&dom, &database());

    assert_eq!(planned.len(), 1);
    assert_eq!(sorted(&planned[0].parts), sorted(&parts));
}

#[test]
fn a_highlight_on_a_part_covers_that_part_alone() {
    let (mut dom, _, _, _, loose) = place();
    insert(&mut dom, 9, "Highlight", Some(loose), Vec::new());

    let planned = plan(&dom, &database());

    assert_eq!(planned[0].parts, vec![loose]);
}

#[test]
fn an_adornee_wins_over_the_parent_wherever_the_highlight_hangs() {
    let (mut dom, workspace, _, _, loose) = place();
    // Parented outside the Workspace entirely, the way a Tool's highlight is.
    let storage = insert(&mut dom, 8, "ReplicatedStorage", None, Vec::new());
    insert(
        &mut dom,
        9,
        "Highlight",
        Some(storage),
        vec![("Adornee", Variant::Ref(loose))],
    );
    // The Workspace itself is never the adornee here, so nothing else is picked up.
    assert!(dom.get(workspace).is_some());

    let planned = plan(&dom, &database());

    assert_eq!(planned[0].parts, vec![loose]);
}

#[test]
fn an_adornee_the_dom_no_longer_holds_falls_back_to_the_parent() {
    let (mut dom, _, model, parts, _) = place();
    insert(
        &mut dom,
        9,
        "Highlight",
        Some(model),
        vec![("Adornee", Variant::Ref(Ref::new(404)))],
    );

    let planned = plan(&dom, &database());

    assert_eq!(sorted(&planned[0].parts), sorted(&parts));
}

#[test]
fn a_disabled_highlight_is_left_out_entirely() {
    let (mut dom, _, model, _, _) = place();
    insert(
        &mut dom,
        9,
        "Highlight",
        Some(model),
        vec![("Enabled", Variant::Bool(false))],
    );

    assert!(plan(&dom, &database()).is_empty());
}

#[test]
fn a_highlight_over_nothing_drawable_is_left_out_too() {
    let (mut dom, workspace, _, _, _) = place();
    let empty = insert(&mut dom, 7, "Folder", Some(workspace), Vec::new());
    insert(&mut dom, 9, "Highlight", Some(empty), Vec::new());

    assert!(plan(&dom, &database()).is_empty());
}

#[test]
fn colours_arrive_linear_and_transparencies_arrive_inverted() {
    let (mut dom, _, _, _, loose) = place();
    insert(
        &mut dom,
        9,
        "Highlight",
        Some(loose),
        vec![
            (
                "FillColor",
                Variant::Color3(Color3Data {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                }),
            ),
            ("FillTransparency", Variant::Float32(0.25)),
            ("OutlineColor", Variant::Color3uint8 { r: 0, g: 255, b: 0 }),
            ("OutlineTransparency", Variant::Float32(1.0)),
        ],
    );

    let planned = plan(&dom, &database());

    assert_eq!(planned[0].fill, [1.0, 0.0, 0.0]);
    assert_eq!(planned[0].outline, [0.0, 1.0, 0.0]);
    assert!((planned[0].fill_alpha - 0.75).abs() < 1e-6);
    assert_eq!(planned[0].outline_alpha, 0.0);
}

#[test]
fn the_defaults_are_an_opaque_white_fill_and_outline_drawn_on_top() {
    let (mut dom, _, _, _, loose) = place();
    insert(&mut dom, 9, "Highlight", Some(loose), Vec::new());

    let planned = plan(&dom, &database());

    assert_eq!(planned[0].fill, [1.0, 1.0, 1.0]);
    assert_eq!(planned[0].outline, [1.0, 1.0, 1.0]);
    assert_eq!(planned[0].fill_alpha, 1.0);
    assert_eq!(planned[0].outline_alpha, 1.0);
    assert_eq!(planned[0].depth_mode, DepthMode::AlwaysOnTop);
}

#[test]
fn depth_mode_one_is_the_occluded_half_of_the_enum() {
    let (mut dom, _, _, _, loose) = place();
    insert(
        &mut dom,
        9,
        "Highlight",
        Some(loose),
        vec![("DepthMode", Variant::Enum(1))],
    );

    assert_eq!(plan(&dom, &database())[0].depth_mode, DepthMode::Occluded);
}

#[test]
fn no_more_than_the_documented_255_are_planned() {
    let (mut dom, _, _, _, loose) = place();
    for id in 0..MAX_HIGHLIGHTS as u32 + 10 {
        insert(&mut dom, 100 + id, "Highlight", Some(loose), Vec::new());
    }

    assert_eq!(plan(&dom, &database()).len(), MAX_HIGHLIGHTS);
}
