//! Where a part's or model's pivot stands in the world, and moving an
//! instance so its pivot lands somewhere else: `PVInstance:GetPivot` and
//! `PVInstance:PivotTo`, and `Model:ScaleTo` around that pivot (see
//! `scale`). Shared by Luau (see `instance`) and the editor's
//! Properties panel, whose `Origin` row reads and writes the same thing.
//!
//! A part's pivot is its `CFrame` times its `PivotOffset`. A model's is its
//! `PrimaryPart`'s when it has one, else its `WorldPivot`, else, a file from
//! before pivots, which stores none, the centre of its parts' bounds, where
//! Studio's pivot Reset puts it.
//!
//! Moving the pivot itself, with the instance left where it is, is
//! [`set_pivot`] and [`reset`], see `edit`.

use std::collections::HashSet;

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::datatypes::cframe::LuaCFrame;

const CFRAME: &str = "CFrame";
const PIVOT_OFFSET: &str = "PivotOffset";
const WORLD_PIVOT: &str = "WorldPivot";
const PRIMARY_PART: &str = "PrimaryPart";
const SIZE: &str = "Size";
const BASE_PART: &str = "BasePart";
const MODEL: &str = "Model";

mod edit;
mod scale;

pub use edit::{clear_primary_part, follow, followers, keep_pivots, reset, set_pivot, Follower};
pub use scale::{scale, scale_by, scale_to};

/// `reference`'s pivot in world space, if it is a part or a model.
pub fn pivot(dom: &WeakDom, db: &ReflectionDatabase, reference: Ref) -> Option<CFrameData> {
    let instance = dom.get(reference)?;
    let class = instance.class();
    if db.is_subclass_of(class, BASE_PART) {
        let frame = cframe(dom, db, reference, CFRAME)?;
        return Some(match cframe(dom, db, reference, PIVOT_OFFSET) {
            Some(offset) if offset != IDENTITY => compose(&frame, &offset),
            _ => frame,
        });
    }
    if !db.is_subclass_of(class, MODEL) {
        return None;
    }
    if let Some(primary) = primary_part(dom, db, reference) {
        return pivot(dom, db, primary);
    }
    if let Some(Variant::CFrame(world)) = instance.properties().get(WORLD_PIVOT) {
        return Some(*world);
    }
    bounds_centre(dom, db, reference, &IDENTITY.rotation)
}

/// `model`'s `PrimaryPart`, while it names a part that still exists.
fn primary_part(dom: &WeakDom, db: &ReflectionDatabase, model: Ref) -> Option<Ref> {
    match dom.get(model)?.properties().get(PRIMARY_PART) {
        Some(Variant::Ref(primary))
            if dom
                .get(*primary)
                .is_some_and(|part| db.is_subclass_of(part.class(), BASE_PART)) =>
        {
            Some(*primary)
        }
        _ => None,
    }
}

/// `PivotTo`: moves `reference` so its pivot lands on `to`, carrying every
/// part under a model, and the model's own `WorldPivot`, by the same move.
/// `None` when `reference` has no pivot (it is neither a part nor a model).
pub fn pivot_to(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    to: &CFrameData,
) -> Option<Result<(), String>> {
    let from = pivot(dom, db, reference)?;
    Some(move_to(dom, db, reference, &from, to, &mut HashSet::new()))
}

/// Carries `reference`: a part, or every part and model under a model,
/// itself included, by what takes its pivot from `from` to `to`: a part's
/// `CFrame` and a model's `WorldPivot`, as Roblox's `PivotTo` transforms
/// every descendant `PVInstance`. A model that stores no `WorldPivot` (a
/// file from before pivots) is given its pivot carried, so `GetPivot`
/// afterwards reads back `to` rather than its parts' recomputed bounds.
/// Everything is read before anything is written, so the bounds a nested
/// model falls back on are the ones from before the move.
pub fn move_to(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    from: &CFrameData,
    to: &CFrameData,
    moved: &mut HashSet<Ref>,
) -> Result<(), String> {
    let mut parts: Vec<Ref> = vec![reference];
    let mut index = 0;
    while let Some(&current) = parts.get(index) {
        index += 1;
        if let Some(instance) = dom.get(current) {
            parts.extend_from_slice(instance.children());
        }
    }
    let mut writes = Vec::new();
    for part in parts {
        let Some(instance) = dom.get(part) else {
            continue;
        };
        let class = instance.class();
        let key = if db.is_subclass_of(class, BASE_PART) {
            CFRAME
        } else if db.is_subclass_of(class, MODEL) {
            WORLD_PIVOT
        } else {
            continue;
        };
        if !moved.insert(part) {
            continue;
        }
        let (key, frame) = match db.stored_or_default(instance, key) {
            Some((key, Variant::CFrame(frame))) => (key, *frame),
            _ if key == WORLD_PIVOT => match pivot(dom, db, part) {
                Some(frame) => (WORLD_PIVOT, frame),
                None => continue,
            },
            _ => continue,
        };
        writes.push((part, key.to_owned(), carried(&frame, from, to)));
    }
    for (part, key, frame) in writes {
        dom.set_property(part, &key, Variant::CFrame(frame))
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// `frame` carried along with a pivot moving from `from` to `to`. Only the
/// position changing, the panel leaves a rotation it did not change
/// byte-identical, moves `frame` by the difference alone, so no rotation
/// passes through an inverse and comes back a rounding error off.
fn carried(frame: &CFrameData, from: &CFrameData, to: &CFrameData) -> CFrameData {
    if from.rotation == to.rotation {
        let (p, t, f) = (&frame.position, &to.position, &from.position);
        return CFrameData {
            position: Vector3Data {
                x: p.x + (t.x - f.x),
                y: p.y + (t.y - f.y),
                z: p.z + (t.z - f.z),
            },
            rotation: frame.rotation,
        };
    }
    compose(&compose(to, &LuaCFrame(*from).inverse().0), frame)
}

/// The centre of the bounds of every part under `reference`, the box
/// squared to `rotation` (and facing that way) rather than to the world,
/// `Model:GetBoundingBox` orients a model's box by its pivot.
fn bounds_centre(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    rotation: &[f32; 9],
) -> Option<CFrameData> {
    let axes = LuaCFrame(CFrameData {
        position: IDENTITY.position,
        rotation: *rotation,
    });
    let into = axes.inverse();
    let mut pending = vec![reference];
    let (mut low, mut high) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    while let Some(current) = pending.pop() {
        let Some(instance) = dom.get(current) else {
            continue;
        };
        pending.extend_from_slice(instance.children());
        if !db.is_subclass_of(instance.class(), BASE_PART) {
            continue;
        }
        let (Some(frame), Some((_, Variant::Vector3(size)))) = (
            cframe(dom, db, current, CFRAME),
            db.stored_or_default(instance, SIZE),
        ) else {
            continue;
        };
        // The part as the box's own axes see it.
        let frame = into.compose(&LuaCFrame(frame)).0;
        // A box's world extent along each axis is its half-size projected
        // through the absolute rotation.
        let absolute = CFrameData {
            position: IDENTITY.position,
            rotation: frame.rotation.map(f32::abs),
        };
        let half = Vector3Data {
            x: size.x * 0.5,
            y: size.y * 0.5,
            z: size.z * 0.5,
        };
        let reach = LuaCFrame(absolute).rotate(half);
        let (centre, reach) = (frame.position, [reach.x, reach.y, reach.z]);
        for (axis, centre) in [centre.x, centre.y, centre.z].into_iter().enumerate() {
            low[axis] = low[axis].min(centre - reach[axis]);
            high[axis] = high[axis].max(centre + reach[axis]);
        }
    }
    (low[0] <= high[0]).then(|| CFrameData {
        position: axes.rotate(Vector3Data {
            x: (low[0] + high[0]) * 0.5,
            y: (low[1] + high[1]) * 0.5,
            z: (low[2] + high[2]) * 0.5,
        }),
        rotation: *rotation,
    })
}

fn cframe(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    name: &str,
) -> Option<CFrameData> {
    match db.stored_or_default(dom.get(reference)?, name)? {
        (_, Variant::CFrame(frame)) => Some(*frame),
        _ => None,
    }
}

const IDENTITY: CFrameData = CFrameData {
    position: Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
    rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
};

/// `a * b`, through the same composition Luau's `CFrame * CFrame` uses.
fn compose(a: &CFrameData, b: &CFrameData) -> CFrameData {
    LuaCFrame(*a).compose(&LuaCFrame(*b)).0
}

#[cfg(test)]
mod tests {
    use rbx_dom::Instance;

    use super::*;

    fn at(x: f32, y: f32, z: f32) -> CFrameData {
        CFrameData {
            position: Vector3Data { x, y, z },
            rotation: IDENTITY.rotation,
        }
    }

    #[test]
    fn pivot_to_moves_a_part_by_its_pivot_not_its_centre() {
        let db = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let part = Ref::new(1);
        let mut instance = Instance::new(part, "Part", "Part");
        instance
            .properties_mut()
            .insert(CFRAME.into(), Variant::CFrame(at(0.0, 0.0, 0.0)));
        instance
            .properties_mut()
            .insert(PIVOT_OFFSET.into(), Variant::CFrame(at(0.0, -2.0, 0.0)));
        dom.insert(instance);

        assert_eq!(pivot(&dom, &db, part), Some(at(0.0, -2.0, 0.0)));
        pivot_to(&mut dom, &db, part, &at(5.0, 0.0, 0.0))
            .expect("a part has a pivot")
            .unwrap();
        assert_eq!(pivot(&dom, &db, part), Some(at(5.0, 0.0, 0.0)));
        assert_eq!(
            dom.get(part).unwrap().properties().get(CFRAME),
            Some(&Variant::CFrame(at(5.0, 2.0, 0.0)))
        );
    }

    #[test]
    fn something_that_is_neither_part_nor_model_has_no_pivot() {
        let db = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let folder = dom.new_instance("Folder", "Folder", None);
        assert_eq!(pivot(&dom, &db, folder), None);
        assert!(pivot_to(&mut dom, &db, folder, &at(1.0, 1.0, 1.0)).is_none());
    }
}
