use rbx_dom::Instance;

use super::*;

const MODEL_REF: Ref = Ref::new(1);
const LEFT: Ref = Ref::new(2);
const RIGHT: Ref = Ref::new(3);

fn db() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

fn at(x: f32, y: f32, z: f32) -> CFrameData {
    CFrameData {
        position: Vector3Data { x, y, z },
        rotation: IDENTITY.rotation,
    }
}

/// A turned rotation whose matrix round-trips through degrees inexactly.
const TURNED: [f32; 9] = [0.6, 0.0, 0.8, 0.0, 1.0, 0.0, -0.8, 0.0, 0.6];

fn part(reference: Ref, frame: CFrameData, extra: &[(&str, Variant)]) -> Instance {
    let mut part = Instance::new(reference, "Part", "Part");
    let properties = part.properties_mut();
    properties.insert("CFrame".into(), Variant::CFrame(frame));
    properties.insert(
        "size".into(),
        Variant::Vector3(Vector3Data {
            x: 2.0,
            y: 2.0,
            z: 2.0,
        }),
    );
    for (key, value) in extra {
        properties.insert((*key).into(), value.clone());
    }
    part
}

/// A model holding two parts at x = -4 and x = 6, and whatever `model`
/// stores itself.
fn model(model: &[(&str, Variant)]) -> WeakDom {
    let mut dom = WeakDom::new();
    let mut root = Instance::new(MODEL_REF, "Model", "Model");
    for (key, value) in model {
        root.properties_mut().insert((*key).into(), value.clone());
    }
    dom.insert(root);
    for (reference, x) in [(LEFT, -4.0), (RIGHT, 6.0)] {
        dom.insert(part(reference, at(x, 1.0, 0.0), &[]));
        dom.set_parent(reference, Some(MODEL_REF));
    }
    dom
}

fn frame_of(dom: &WeakDom, reference: Ref, key: &str) -> CFrameData {
    match dom.get(reference).unwrap().properties().get(key) {
        Some(Variant::CFrame(frame)) => *frame,
        other => panic!("{key}: {other:?}"),
    }
}

#[test]
fn a_part_s_pivot_is_its_cframe_times_its_offset() {
    let mut dom = WeakDom::new();
    let offset = Variant::CFrame(at(0.0, 1.0, 0.0));
    dom.insert(part(LEFT, at(1.0, 2.0, 3.0), &[("PivotOffset", offset)]));

    assert_eq!(pivot(&dom, &db(), LEFT), Some(at(1.0, 3.0, 3.0)));
}

#[test]
fn a_model_without_a_pivot_of_its_own_pivots_on_its_bounds() {
    // Parts span x = -5 .. 7 and y = 0 .. 2.
    assert_eq!(
        pivot(&model(&[]), &db(), MODEL_REF),
        Some(at(1.0, 1.0, 0.0))
    );

    let world = Variant::CFrame(at(9.0, 9.0, 9.0));
    let dom = model(&[("WorldPivot", world)]);
    assert_eq!(pivot(&dom, &db(), MODEL_REF), Some(at(9.0, 9.0, 9.0)));

    let dom = model(&[("PrimaryPart", Variant::Ref(RIGHT))]);
    assert_eq!(pivot(&dom, &db(), MODEL_REF), Some(at(6.0, 1.0, 0.0)));
}

#[test]
fn typing_an_origin_moves_the_part_and_keeps_its_rotation_exact() {
    let mut dom = WeakDom::new();
    let turned = CFrameData {
        position: Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        rotation: TURNED,
    };
    dom.insert(part(LEFT, turned, &[]));
    let db = db();
    let shown = super::super::edit_text(&Variant::CFrame(turned)).unwrap();
    let angles = shown.splitn(4, ", ").nth(3).unwrap();

    pivot_all(&mut dom, &db, &[LEFT], &format!("10, 0, 0, {angles}")).unwrap();

    let moved = frame_of(&dom, LEFT, "CFrame");
    assert_eq!(moved.position, at(10.0, 0.0, 0.0).position);
    assert_eq!(moved.rotation, TURNED);
}

#[test]
fn typing_a_model_s_origin_carries_every_part_and_its_world_pivot() {
    let world = Variant::CFrame(at(1.0, 1.0, 0.0));
    let mut dom = model(&[("WorldPivot", world)]);

    pivot_all(&mut dom, &db(), &[MODEL_REF], "1, 11, 0, 0, 0, 0").unwrap();

    assert_eq!(frame_of(&dom, LEFT, "CFrame"), at(-4.0, 11.0, 0.0));
    assert_eq!(frame_of(&dom, RIGHT, "CFrame"), at(6.0, 11.0, 0.0));
    assert_eq!(frame_of(&dom, MODEL_REF, "WorldPivot"), at(1.0, 11.0, 0.0));
}

#[test]
fn a_part_selected_with_its_model_moves_once() {
    let mut dom = model(&[]);

    // Blank fields keep each instance's own value: only Y is typed.
    pivot_all(&mut dom, &db(), &[MODEL_REF, LEFT], ", 5, , , , ").unwrap();

    assert_eq!(frame_of(&dom, LEFT, "CFrame"), at(-4.0, 5.0, 0.0));
    assert_eq!(frame_of(&dom, RIGHT, "CFrame"), at(6.0, 5.0, 0.0));
}

#[test]
fn turning_the_origin_turns_the_part_about_its_pivot() {
    let mut dom = WeakDom::new();
    let offset = Variant::CFrame(at(2.0, 0.0, 0.0));
    dom.insert(part(LEFT, at(0.0, 0.0, 0.0), &[("PivotOffset", offset)]));

    // A quarter turn about Y at the pivot (2, 0, 0) swings the part's
    // centre from 2 studs -X of the pivot to 2 studs +Z of it.
    pivot_all(&mut dom, &db(), &[LEFT], "2, 0, 0, 0, 90, 0").unwrap();

    let moved = frame_of(&dom, LEFT, "CFrame").position;
    assert!((moved.x - 2.0).abs() < 1e-4, "{moved:?}");
    assert!((moved.z - 2.0).abs() < 1e-4, "{moved:?}");
}
