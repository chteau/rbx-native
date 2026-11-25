//! `Model:ScaleTo` and `Model:GetScale` (`reference/engine/classes/Model.yaml`):
//! a model keeps a canonical scale factor, saved as `ScaleFactor` and 1 for
//! a new model, and `ScaleTo` resizes and moves everything under the model
//! around its pivot so it stands at the new factor relative to factor 1.
//!
//! Scaled here: part sizes and positions, part `PivotOffset`s, nested model
//! pivots, attachment and bone offsets, joint `C0`/`C1` offsets and a file
//! mesh's `Scale` and `Offset`. Roblox also scales constraint lengths,
//! forces, particle sizes and sound distances, which this leaves as they
//! are. Roblox caches each descendant's size at factor 1 for the session so
//! a round trip comes back with no drift; this scales by the ratio of the
//! two factors instead, so repeated calls can drift by float rounding.

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{pivot, BASE_PART, CFRAME, MODEL, PIVOT_OFFSET, SIZE, WORLD_PIVOT};

/// The saved scale factor's name as the API knows it (saved as `ScaleFactor`).
const SCALE: &str = "Scale";
/// `MeshType.FileMesh`: only a file mesh's `Scale` is in studs; the other
/// mesh types scale relative to the part's own size.
const FILE_MESH: u32 = 5;

/// `Model:GetScale`: the model's canonical scale factor, `None` when
/// `model` is not a model.
pub fn scale(dom: &WeakDom, db: &ReflectionDatabase, model: Ref) -> Option<f32> {
    let instance = dom.get(model)?;
    if !db.is_subclass_of(instance.class(), MODEL) {
        return None;
    }
    Some(match db.stored_or_default(instance, SCALE) {
        Some((_, Variant::Float32(scale))) if *scale > 0.0 => *scale,
        _ => 1.0,
    })
}

/// `Model:ScaleTo`: scales `model` around its pivot from its current factor
/// to `factor` and saves `factor`. `None` when `model` is not a model.
pub fn scale_to(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    model: Ref,
    factor: f32,
) -> Option<Result<(), String>> {
    let current = scale(dom, db, model)?;
    if !factor.is_finite() || factor <= 0.0 {
        return Some(Err(
            "ScaleTo: the scale factor must be a positive, non-zero number".to_owned(),
        ));
    }
    Some(scale_by(dom, db, model, factor / current).and_then(|()| {
        let key = db
            .stored_or_default(dom.get(model).ok_or("the model is gone")?, SCALE)
            .map_or(SCALE, |(key, _)| key)
            .to_owned();
        dom.set_property(model, &key, Variant::Float32(factor))
            .map(|_| ())
            .map_err(|err| err.to_string())
    }))
}

/// Resizes and moves everything under `model` by `ratio` around the
/// model's pivot, which stays where it is, without touching its saved
/// factor: what an importer bringing a file to studs wants, since the
/// imported size is the model's size at factor 1.
pub fn scale_by(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    model: Ref,
    ratio: f32,
) -> Result<(), String> {
    let centre = pivot(dom, db, model).ok_or("only a part or model has a pivot")?;
    let mut pending = vec![model];
    let mut writes: Vec<(Ref, String, Variant)> = Vec::new();
    while let Some(current) = pending.pop() {
        let Some(instance) = dom.get(current) else {
            continue;
        };
        pending.extend_from_slice(instance.children());
        let class = instance.class();
        let mut write = |name: &str, change: &dyn Fn(&Variant) -> Option<Variant>| {
            if let Some((key, value)) = db.stored_or_default(instance, name) {
                if let Some(new) = change(value) {
                    writes.push((current, key.to_owned(), new));
                }
            }
        };
        if db.is_subclass_of(class, BASE_PART) {
            write(SIZE, &|v| {
                vector(v).map(|s| Variant::Vector3(times(s, ratio)))
            });
            write(CFRAME, &|v| about(v, &centre.position, ratio));
            write(PIVOT_OFFSET, &|v| local(v, ratio));
        } else if db.is_subclass_of(class, MODEL) && current != model {
            // A nested model with no stored pivot reads its parts' bounds,
            // which scale with the parts themselves.
            write(WORLD_PIVOT, &|v| about(v, &centre.position, ratio));
        } else if db.is_subclass_of(class, "Attachment") {
            write(CFRAME, &|v| local(v, ratio));
        } else if db.is_subclass_of(class, "JointInstance") {
            write("C0", &|v| local(v, ratio));
            write("C1", &|v| local(v, ratio));
        } else if class == "SpecialMesh"
            && matches!(
                db.stored_or_default(instance, "MeshType"),
                Some((_, Variant::Enum(FILE_MESH)))
            )
        {
            write(SCALE, &|v| {
                vector(v).map(|s| Variant::Vector3(times(s, ratio)))
            });
            write("Offset", &|v| {
                vector(v).map(|s| Variant::Vector3(times(s, ratio)))
            });
        }
    }
    for (target, key, value) in writes {
        dom.set_property(target, &key, value)
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn vector(value: &Variant) -> Option<Vector3Data> {
    match value {
        Variant::Vector3(v) => Some(*v),
        _ => None,
    }
}

fn times(v: Vector3Data, k: f32) -> Vector3Data {
    Vector3Data {
        x: v.x * k,
        y: v.y * k,
        z: v.z * k,
    }
}

/// A world `CFrame` (or a set `OptionalCFrame`, as a model's pivot may be
/// stored) whose position moves away from `centre` by `ratio`.
fn about(value: &Variant, centre: &Vector3Data, ratio: f32) -> Option<Variant> {
    let away = |frame: &CFrameData| {
        let p = &frame.position;
        CFrameData {
            position: Vector3Data {
                x: centre.x + (p.x - centre.x) * ratio,
                y: centre.y + (p.y - centre.y) * ratio,
                z: centre.z + (p.z - centre.z) * ratio,
            },
            rotation: frame.rotation,
        }
    };
    match value {
        Variant::CFrame(frame) => Some(Variant::CFrame(away(frame))),
        Variant::OptionalCFrame(Some(frame)) => Some(Variant::OptionalCFrame(Some(away(frame)))),
        _ => None,
    }
}

/// An offset `CFrame` whose position scales by `ratio`.
fn local(value: &Variant, ratio: f32) -> Option<Variant> {
    let Variant::CFrame(frame) = value else {
        return None;
    };
    Some(Variant::CFrame(CFrameData {
        position: times(frame.position, ratio),
        rotation: frame.rotation,
    }))
}

#[cfg(test)]
#[path = "scale/tests.rs"]
mod tests;
