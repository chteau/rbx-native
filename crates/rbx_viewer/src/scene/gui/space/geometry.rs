//! Where a canvas' rectangle is in the world and how many pixels it gets:
//! the face of a part a `SurfaceGui` covers, and the stud size a
//! `BillboardGui` asks for.

use std::collections::BTreeMap;

use glam::Vec3;
use rbx_dom::Variant;

use super::super::plan::{float, vector2, Span};
use super::{DEFAULT_CANVAS, FIXED_SIZE, MAX_CANVAS, PIXELS_PER_STUD};
use crate::scene::Placement;
use crate::textures::NormalId;

/// `Face`, Front by default like Roblox itself.
pub(in crate::scene::gui) fn face(properties: &BTreeMap<String, Variant>) -> NormalId {
    match properties.get("Face") {
        Some(&Variant::Enum(raw)) => NormalId::from_ordinal(raw).unwrap_or(NormalId::Front),
        _ => NormalId::Front,
    }
}

/// The canvas' pixel size for a billboard of `size` studs.
pub(in crate::scene::gui) fn billboard_canvas(size: [f32; 2]) -> [f32; 2] {
    size.map(|studs| (studs * PIXELS_PER_STUD).clamp(0.0, MAX_CANVAS).round())
}

/// The pixel size of a `SurfaceGui`'s canvas: the face's stud size at
/// `PixelsPerStud` by default, `CanvasSize` under `SizingMode.FixedSize`.
/// Either way the canvas is stretched over the whole face, so the two only
/// differ in how many pixels a `UDim2` offset comes to.
///
/// Falls back to [`DEFAULT_CANVAS`] per axis where the result is degenerate —
/// a zero axis would ask for a texture no adapter will allocate.
pub(in crate::scene::gui) fn surface_canvas(
    properties: &BTreeMap<String, Variant>,
    face_studs: [f32; 2],
) -> [f32; 2] {
    let raw = match properties.get("SizingMode") {
        Some(&Variant::Enum(FIXED_SIZE)) => vector2(properties, "CanvasSize"),
        _ => {
            let density = float(properties, "PixelsPerStud", PIXELS_PER_STUD);
            face_studs.map(|studs| studs * density)
        }
    };
    let axis = |value: f32, default: f32| match value.is_finite() && value >= 1.0 {
        true => value.min(MAX_CANVAS).round(),
        false => default,
    };
    [
        axis(raw[0], DEFAULT_CANVAS[0]),
        axis(raw[1], DEFAULT_CANVAS[1]),
    ]
}

/// World width and height of a `BillboardGui.Size`, in studs.
///
/// Simplification: Roblox gives the two halves of that `UDim2` different
/// units — the scale half is the billboard's stud size in 3D, the offset half
/// a constant screen-pixel size that does not shrink with distance. Only the
/// first is reproduced; an offset-only `Size` is read as studs at
/// [`PIXELS_PER_STUD`], so such a billboard keeps a fixed *world* size instead
/// of a fixed *screen* one.
///
/// TODO: true scale-with-distance for the offset half.
pub(in crate::scene::gui) fn studs(size: Span) -> [f32; 2] {
    let axis = |scale: f32, offset: f32| match scale > 0.0 {
        true => scale,
        false => (offset / PIXELS_PER_STUD).max(0.0),
    };
    [
        axis(size.scale[0], size.offset[0]),
        axis(size.scale[1], size.offset[1]),
    ]
}

/// The four world corners of `face` on a part, in image order.
///
/// The same rectangle a stretched `Decal` covers: [`NormalId::axes`] is what
/// `crate::textures::face` builds its own projection from, so the canvas and a
/// decal on the very same face land on exactly the same quad.
pub(in crate::scene::gui) fn face_corners(
    face: NormalId,
    placement: &Placement,
    z_offset: f32,
) -> [Vec3; 4] {
    let (normal, u, v) = face.axes();
    // The unit mesh spans [-0.5, 0.5]³, so the face plane sits half a unit
    // along its own normal and the image axes span the other two.
    let centre = normal * 0.5;
    let model = &placement.model;
    let push = model.transform_vector3(normal).normalize_or_zero() * z_offset;
    let corner = |right: f32, down: f32| {
        model.transform_point3(centre + u * (right * 0.5) + v * (down * 0.5)) + push
    };
    [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ]
}

/// Width and height in studs of a face quad in image order, as the part is
/// actually placed — so a scaled `Placement` sizes the canvas like Roblox
/// sizes it off the part's own `Size`.
pub(in crate::scene::gui) fn face_studs(corners: &[Vec3; 4]) -> [f32; 2] {
    [
        corners[1].distance(corners[0]),
        corners[3].distance(corners[0]),
    ]
}
