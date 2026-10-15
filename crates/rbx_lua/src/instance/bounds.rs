//! `Model:GetBoundingBox`: the box around every part under a model, aligned
//! to the model's pivot frame rather than the world axes.

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::tree;

fn part_property<'a>(
    db: &'a ReflectionDatabase,
    instance: &'a rbx_dom::Instance,
    name: &'a str,
) -> Option<&'a Variant> {
    db.stored_or_default(instance, name).map(|(_, value)| value)
}

/// `model`'s oriented bounds as (centre frame, size); an empty model gives
/// its pivot and a zero size.
pub(super) fn bounding_box(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    model: Ref,
) -> Option<(CFrameData, Vector3Data)> {
    let pivot = crate::pivot::pivot(dom, db, model)?;
    let r = pivot.rotation;
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    for referent in tree::descendants(dom, model) {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        if !db.is_subclass_of(instance.class(), "BasePart") {
            continue;
        }
        let (Some(Variant::CFrame(frame)), Some(Variant::Vector3(size))) = (
            part_property(db, instance, "CFrame"),
            part_property(db, instance, "Size"),
        ) else {
            continue;
        };
        let half = [size.x / 2.0, size.y / 2.0, size.z / 2.0];
        for corner in 0..8 {
            let local = [
                if corner & 1 == 0 { -half[0] } else { half[0] },
                if corner & 2 == 0 { -half[1] } else { half[1] },
                if corner & 4 == 0 { -half[2] } else { half[2] },
            ];
            let m = frame.rotation;
            let world = [
                frame.position.x + m[0] * local[0] + m[1] * local[1] + m[2] * local[2],
                frame.position.y + m[3] * local[0] + m[4] * local[1] + m[5] * local[2],
                frame.position.z + m[6] * local[0] + m[7] * local[1] + m[8] * local[2],
            ];
            let d = [
                world[0] - pivot.position.x,
                world[1] - pivot.position.y,
                world[2] - pivot.position.z,
            ];
            for axis in 0..3 {
                let value = r[axis] * d[0] + r[3 + axis] * d[1] + r[6 + axis] * d[2];
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
    }

    if min[0] > max[0] {
        return Some((
            pivot,
            Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        ));
    }
    let centre = [
        (min[0] + max[0]) / 2.0,
        (min[1] + max[1]) / 2.0,
        (min[2] + max[2]) / 2.0,
    ];
    let position = Vector3Data {
        x: pivot.position.x + r[0] * centre[0] + r[1] * centre[1] + r[2] * centre[2],
        y: pivot.position.y + r[3] * centre[0] + r[4] * centre[1] + r[5] * centre[2],
        z: pivot.position.z + r[6] * centre[0] + r[7] * centre[1] + r[8] * centre[2],
    };
    Some((
        CFrameData {
            position,
            rotation: r,
        },
        Vector3Data {
            x: max[0] - min[0],
            y: max[1] - min[1],
            z: max[2] - min[2],
        },
    ))
}
