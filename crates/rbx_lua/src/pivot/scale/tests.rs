use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{scale, scale_by, scale_to};

fn at(x: f32, y: f32, z: f32) -> CFrameData {
    CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    }
}

/// A model pivoted at the origin holding a 2x2x2 part at (2, 1, 0) with an
/// attachment 1 stud above its centre.
fn model() -> (WeakDom, Ref, Ref, Ref) {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Crate", None);
    dom.set_property(model, "WorldPivot", Variant::CFrame(at(0.0, 0.0, 0.0)))
        .unwrap();
    let part = dom.new_instance("Part", "Box", Some(model));
    dom.set_property(part, "CFrame", Variant::CFrame(at(2.0, 1.0, 0.0)))
        .unwrap();
    dom.set_property(
        part,
        "size",
        Variant::Vector3(Vector3Data {
            x: 2.0,
            y: 2.0,
            z: 2.0,
        }),
    )
    .unwrap();
    let attachment = dom.new_instance("Attachment", "Top", Some(part));
    dom.set_property(attachment, "CFrame", Variant::CFrame(at(0.0, 1.0, 0.0)))
        .unwrap();
    (dom, model, part, attachment)
}

fn frame(dom: &WeakDom, r: Ref) -> Vector3Data {
    match dom.get(r).unwrap().properties().get("CFrame") {
        Some(Variant::CFrame(c)) => c.position,
        other => panic!("no CFrame: {other:?}"),
    }
}

#[test]
fn a_new_model_is_at_scale_one_and_a_part_has_no_scale() {
    let db = ReflectionDatabase::embedded();
    let (dom, model, part, _) = model();
    assert_eq!(scale(&dom, &db, model), Some(1.0));
    assert_eq!(scale(&dom, &db, part), None);
}

#[test]
fn scale_to_grows_sizes_and_offsets_around_the_pivot_and_saves_the_factor() {
    let db = ReflectionDatabase::embedded();
    let (mut dom, model, part, attachment) = model();
    scale_to(&mut dom, &db, model, 2.0).unwrap().unwrap();
    assert_eq!(scale(&dom, &db, model), Some(2.0));
    assert_eq!(
        dom.get(model).unwrap().properties().get("ScaleFactor"),
        Some(&Variant::Float32(2.0))
    );
    assert_eq!(
        frame(&dom, part),
        Vector3Data {
            x: 4.0,
            y: 2.0,
            z: 0.0
        }
    );
    assert_eq!(
        dom.get(part).unwrap().properties().get("size"),
        Some(&Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 4.0,
            z: 4.0
        }))
    );
    assert_eq!(
        frame(&dom, attachment),
        Vector3Data {
            x: 0.0,
            y: 2.0,
            z: 0.0
        }
    );

    // Relative to factor 1, not to the last call: back to 1 is the original.
    scale_to(&mut dom, &db, model, 1.0).unwrap().unwrap();
    assert_eq!(
        frame(&dom, part),
        Vector3Data {
            x: 2.0,
            y: 1.0,
            z: 0.0
        }
    );
}

#[test]
fn scale_by_leaves_the_saved_factor_alone() {
    let db = ReflectionDatabase::embedded();
    let (mut dom, model, part, _) = model();
    scale_by(&mut dom, &db, model, 3.0).unwrap();
    assert_eq!(scale(&dom, &db, model), Some(1.0));
    assert_eq!(
        frame(&dom, part),
        Vector3Data {
            x: 6.0,
            y: 3.0,
            z: 0.0
        }
    );
}

#[test]
fn a_factor_that_is_not_positive_is_refused() {
    let db = ReflectionDatabase::embedded();
    let (mut dom, model, part, _) = model();
    for bad in [0.0, -1.0, f32::NAN] {
        assert!(scale_to(&mut dom, &db, model, bad).unwrap().is_err());
    }
    assert_eq!(
        frame(&dom, part),
        Vector3Data {
            x: 2.0,
            y: 1.0,
            z: 0.0
        }
    );
    assert!(scale_to(&mut dom, &db, part, 2.0).is_none());
}
