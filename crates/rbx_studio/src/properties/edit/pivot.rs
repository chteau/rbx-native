//! `Origin`: the row Studio's Properties window shows for where an instance's
//! pivot stands in the world, "the object based on its pivot point rather
//! than its bounding box" (`studio/pivot-tools.md`) — `PVInstance:GetPivot`
//! — and typing into it moves the instance there, as `PivotTo` does.
//!
//! A part's pivot is its `CFrame` times its `PivotOffset`. A model's is its
//! `PrimaryPart`'s when it has one, else its `WorldPivot`, else — a file from
//! before pivots, which stores none — the centre of its parts' bounds, where
//! Studio's pivot Reset puts it.

use std::collections::HashSet;

use glam::{Affine3A, Mat3, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::many::fill_blanks;
use super::parse;

pub(crate) const ORIGIN: &str = "Origin";
const CFRAME: &str = "CFrame";
const PIVOT_OFFSET: &str = "PivotOffset";
const WORLD_PIVOT: &str = "WorldPivot";
const PRIMARY_PART: &str = "PrimaryPart";
const SIZE: &str = "Size";
const BASE_PART: &str = "BasePart";
const MODEL: &str = "Model";

/// `reference`'s pivot in world space, if it is a part or a model.
pub(crate) fn pivot(dom: &WeakDom, db: &ReflectionDatabase, reference: Ref) -> Option<CFrameData> {
    let instance = dom.get(reference)?;
    let class = instance.class();
    if db.is_subclass_of(class, BASE_PART) {
        let frame = cframe(dom, db, reference, CFRAME)?;
        return Some(match cframe(dom, db, reference, PIVOT_OFFSET) {
            Some(offset) if offset != IDENTITY => to_data(affine(&frame) * affine(&offset)),
            _ => frame,
        });
    }
    if !db.is_subclass_of(class, MODEL) {
        return None;
    }
    if let Some(Variant::Ref(primary)) = instance.properties().get(PRIMARY_PART) {
        if dom
            .get(*primary)
            .is_some_and(|part| db.is_subclass_of(part.class(), BASE_PART))
        {
            return pivot(dom, db, *primary);
        }
    }
    if let Some(Variant::CFrame(world)) = instance.properties().get(WORLD_PIVOT) {
        return Some(*world);
    }
    bounds_centre(dom, db, reference)
}

/// Moves every instance in `selection` so its pivot lands where `text` puts
/// it, as one edit. Where the pivots differ the row showed blank, and what
/// was left blank keeps each instance's own (see `many::commit_all`).
pub(super) fn pivot_all(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    selection: &[Ref],
    text: &str,
) -> Result<(), String> {
    let currents: Vec<CFrameData> = selection
        .iter()
        .map(|&reference| pivot(dom, db, reference).ok_or_else(|| "has no pivot".to_string()))
        .collect::<Result<_, _>>()?;
    let mixed = currents.windows(2).any(|pair| pair[0] != pair[1]);

    // Every value parsed before any is written, so one rejected value moves
    // nothing.
    let mut moves = Vec::with_capacity(selection.len());
    for (&reference, current) in selection.iter().zip(&currents) {
        let value = Variant::CFrame(*current);
        let text = if mixed {
            fill_blanks(&value, text)
        } else {
            text.to_owned()
        };
        let Variant::CFrame(target) = parse(&value, db, "", ORIGIN, &text)? else {
            return Err(format!("{ORIGIN} takes a CFrame"));
        };
        if target != *current {
            moves.push((reference, *current, target));
        }
    }
    // A part selected along with its model moves once, with the model.
    let mut moved = HashSet::new();
    for (reference, from, to) in moves {
        move_pivot(dom, db, reference, &from, &to, &mut moved)?;
    }
    Ok(())
}

/// Carries `reference` — a part, or every part under a model and the
/// model's own `WorldPivot` — by what takes its pivot from `from` to `to`.
fn move_pivot(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    from: &CFrameData,
    to: &CFrameData,
    moved: &mut HashSet<Ref>,
) -> Result<(), String> {
    let carry = |frame: &CFrameData| carried(frame, from, to);
    let mut parts: Vec<Ref> = vec![reference];
    let mut index = 0;
    while let Some(&current) = parts.get(index) {
        index += 1;
        if let Some(instance) = dom.get(current) {
            parts.extend_from_slice(instance.children());
        }
    }
    for part in parts {
        let Some(instance) = dom.get(part) else {
            continue;
        };
        let class = instance.class();
        let key = if db.is_subclass_of(class, BASE_PART) {
            CFRAME
        } else if db.is_subclass_of(class, MODEL) && part == reference {
            WORLD_PIVOT
        } else {
            continue;
        };
        if !moved.insert(part) {
            continue;
        }
        let Some((key, Variant::CFrame(frame))) = db.stored_or_default(instance, key) else {
            continue;
        };
        let (key, frame) = (key.to_owned(), carry(frame));
        dom.set_property(part, &key, Variant::CFrame(frame))
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// `frame` carried along with a pivot moving from `from` to `to`. Only the
/// position changing — the panel leaves a rotation it did not change
/// byte-identical — moves `frame` by the difference alone, so no rotation
/// passes through an inverse and comes back a rounding error off.
fn carried(frame: &CFrameData, from: &CFrameData, to: &CFrameData) -> CFrameData {
    if from.rotation == to.rotation {
        let shift = vec3(&to.position) - vec3(&from.position);
        return CFrameData {
            position: vector(vec3(&frame.position) + shift),
            rotation: frame.rotation,
        };
    }
    to_data(affine(to) * affine(from).inverse() * affine(frame))
}

/// The centre of the world bounds of every part under `reference`, facing
/// the world's axes.
fn bounds_centre(dom: &WeakDom, db: &ReflectionDatabase, reference: Ref) -> Option<CFrameData> {
    let mut pending = vec![reference];
    let (mut low, mut high) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
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
        let frame = affine(&frame);
        let half = vec3(size) * 0.5;
        // A box's world extent along each axis is its half-size projected
        // through the absolute rotation.
        let reach = Mat3::from(frame.matrix3).abs() * half;
        let centre = Vec3::from(frame.translation);
        low = low.min(centre - reach);
        high = high.max(centre + reach);
    }
    (low.x <= high.x).then(|| CFrameData {
        position: vector((low + high) * 0.5),
        rotation: IDENTITY.rotation,
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

/// A `CFrame` as an affine map; its rotation is stored row by row.
fn affine(frame: &CFrameData) -> Affine3A {
    let rows = Mat3::from_cols_array(&frame.rotation);
    Affine3A::from_mat3_translation(rows.transpose(), vec3(&frame.position))
}

fn to_data(map: Affine3A) -> CFrameData {
    CFrameData {
        position: vector(map.translation.into()),
        rotation: Mat3::from(map.matrix3).transpose().to_cols_array(),
    }
}

fn vec3(value: &Vector3Data) -> Vec3 {
    Vec3::new(value.x, value.y, value.z)
}

fn vector(value: Vec3) -> Vector3Data {
    Vector3Data {
        x: value.x,
        y: value.y,
        z: value.z,
    }
}

#[cfg(test)]
mod tests;
