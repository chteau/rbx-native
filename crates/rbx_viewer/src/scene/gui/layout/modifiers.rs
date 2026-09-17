//! The pixel form of an element's `UICorner`/`UIStroke`/`UIGradient`, each
//! resolved against the element's own box the way its docs say.

use rbx_dom::{ColorSequence, NumberSequence};

use super::super::plan::{Corner, Gradient, GradientKind, Join, Stroke, StrokePosition, Tile};

/// A `UIStroke` as a band of signed distances from the element's edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StrokePx {
    pub(crate) color: [f32; 3],
    pub(crate) alpha: f32,
    /// The band's inner and outer edge as signed distances from the box's
    /// outline, negative inside: `Outer` is `[0, thickness]`.
    pub(crate) band: [f32; 2],
    pub(crate) join: Join,
    /// See `plan::Stroke::on_text`: the box painter leaves such a stroke to
    /// the text renderer.
    pub(crate) on_text: bool,
}

/// A `UIGradient` with its geometry turned into what a fragment needs to
/// find its place on the ramp, all in the element's own unrotated frame
/// relative to the box centre.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GradientPx {
    pub(crate) color: ColorSequence,
    pub(crate) transparency: NumberSequence,
    /// `Offset` in pixels from the centre.
    pub(crate) origin: [f32; 2],
    /// Per [`GradientKind`]: `Linear`, the ramp direction scaled so that a
    /// dot product with (position − origin) moves `t` by exactly the box's
    /// extent along it, `t` being 0.5 at the origin; `Radial`, `1 / radius`
    /// in `x`; `Conical`, the start angle in radians in `x` and the sweep's
    /// reciprocal in `y`.
    pub(crate) axis: [f32; 2],
    pub(crate) kind: GradientKind,
    pub(crate) tile: Tile,
}

/// The four radii in pixels, top-left first then clockwise. Per the docs a
/// radius' scale is a fraction of the *shorter* side, and anything past half
/// of it is a pill — so that is where each is clamped, which also keeps two
/// radii on one side from ever overlapping.
pub(super) fn radii(corner: Option<&Corner>, size: [f32; 2]) -> [f32; 4] {
    let Some(corner) = corner else {
        return [0.0; 4];
    };
    let shorter = size[0].min(size[1]).max(0.0);
    corner
        .radii
        .map(|(scale, offset)| (scale * shorter + offset).clamp(0.0, shorter * 0.5))
}

pub(super) fn stroke(stroke: &Stroke, size: [f32; 2]) -> StrokePx {
    let shorter = size[0].min(size[1]).max(0.0);
    let thickness = match stroke.scaled {
        true => stroke.thickness * shorter,
        false => stroke.thickness,
    };
    let shift = stroke.offset.0 * shorter + stroke.offset.1;
    let (inner, outer) = match stroke.position {
        StrokePosition::Outer => (0.0, thickness),
        StrokePosition::Center => (-thickness * 0.5, thickness * 0.5),
        StrokePosition::Inner => (-thickness, 0.0),
    };
    StrokePx {
        color: stroke.color,
        alpha: stroke.alpha,
        band: [inner + shift, outer + shift],
        join: stroke.join,
        on_text: stroke.on_text,
    }
}

pub(super) fn gradient(gradient: &Gradient, size: [f32; 2]) -> GradientPx {
    let radians = gradient.rotation.to_radians();
    let (sin, cos) = radians.sin_cos();
    let axis = match gradient.kind {
        GradientKind::Linear => {
            // "The beginning and end control points snap to the edges of the
            // parent": the ramp spans the box's projection onto its own
            // direction, and `Scale` stretches that span about the centre.
            // The docs do not say where `Scale` is anchored; the centre is
            // what keeps `Offset`'s "translation from the centre" wording
            // true at every scale.
            let span = (size[0] * cos).abs() + (size[1] * sin).abs();
            let scaled = (span * gradient.scale).max(f32::EPSILON);
            [cos / scaled, sin / scaled]
        }
        // "The radius is defined by the average of the element's width and
        // height divided by two, effectively (width+height)/4."
        GradientKind::Radial => {
            let radius = ((size[0] + size[1]) * 0.25 * gradient.scale).max(f32::EPSILON);
            [1.0 / radius, 0.0]
        }
        // `Scale` shrinks the sweep below a full turn but, per the docs,
        // never stretches it past one.
        GradientKind::Conical => [radians, 1.0 / gradient.scale.min(1.0)],
    };
    GradientPx {
        color: gradient.color.clone(),
        transparency: gradient.transparency.clone(),
        origin: [gradient.offset[0] * size[0], gradient.offset[1] * size[1]],
        axis,
        kind: gradient.kind,
        tile: gradient.tile,
    }
}
