//! Where a transform drag is allowed to land: on a grid increment, or pulled
//! onto the surface or edge of a part it passes near.
//!
//! Pure geometry, no GPU and no DOM, for the same reason [`crate::gizmo`] is:
//! the embedder drags on the UI thread against the boxes the renderer draws,
//! so both halves have to agree on where a part ends up.
//!
//! `creator-docs` (`parts/index.md#transform-parts`) gives the two behaviours
//! separately:
//!
//! > Tool transform **snapping** increments are based on **studs** for
//! > moving/scaling or **degrees** for rotating, each adjustable in the
//! > toolbar.
//!
//! > If snapping is **disabled**, the part will "soft snap" to surfaces and
//! > edges of nearby parts.
//!
//! What the docs never give is a number: neither how a grid is anchored nor
//! how near "nearby" is. Both are this module's own choice, and each is
//! documented where it is made rather than presented as Studio's.

use glam::{Mat4, Vec3};

/// Rounds to the nearest multiple of `increment`, halves away from zero.
///
/// A zero, negative or non-finite increment is no grid at all and passes the
/// value through: the toolbar's field accepts whatever is typed into it, and
/// "0 studs" has to mean "don't snap" rather than "divide by zero".
pub fn round_to(value: f32, increment: f32) -> f32 {
    if increment <= 0.0 || !increment.is_finite() {
        return value;
    }
    (value / increment).round() * increment
}

/// [`round_to`] on all three components — a drag's travel rounded onto the
/// grid.
pub fn round_point(point: Vec3, increment: f32) -> Vec3 {
    Vec3::new(
        round_to(point.x, increment),
        round_to(point.y, increment),
        round_to(point.z, increment),
    )
}

/// A part scaled to nothing on some axis has no surface to snap to, and its
/// basis cannot be normalized into one either.
const DEGENERATE: f32 = 1e-6;

/// A point on some part's surface, and which way that surface faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    pub point: Vec3,
    /// Outward, unit length — the face's own normal, which is what a 90°
    /// `R` turn while cursor-dragging rotates about.
    pub normal: Vec3,
}

/// The nearest point on the surface of any of `boxes` to `point`, if one is
/// within `reach` studs of it — Studio's "soft snap".
///
/// `boxes` are the oriented boxes parts are drawn inside, the same matrices
/// [`crate::pick::model_of`] builds. Each is measured in its own rigid frame
/// rather than by clamping into unit-cube space, so a long thin part's
/// surface is the same distance away here as it looks on screen.
///
/// An edge wins over the face it belongs to: once the nearest surface point
/// is already within `reach` of a second face as well, it is pulled onto the
/// shared edge, and onto a corner if a third is in reach too. That is the
/// whole of what "surfaces and edges" means here — the docs describe the
/// behaviour and show it, but publish no threshold, so `reach` is the
/// caller's to choose (see `workspace_view::gizmo`).
pub fn nearest_surface(point: Vec3, boxes: &[Mat4], reach: f32) -> Option<Surface> {
    boxes
        .iter()
        .filter_map(|model| surface_of(point, *model, reach))
        .map(|surface| ((surface.point - point).length(), surface))
        .filter(|(distance, _)| *distance <= reach)
        .min_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, surface)| surface)
}

/// [`nearest_surface`] against one box.
fn surface_of(point: Vec3, model: Mat4, reach: f32) -> Option<Surface> {
    let mut axes = [Vec3::ZERO; 3];
    let mut half = Vec3::ZERO;
    for (index, column) in [model.x_axis, model.y_axis, model.z_axis]
        .into_iter()
        .enumerate()
    {
        let column = column.truncate();
        let length = column.length();
        if length < DEGENERATE {
            return None;
        }
        axes[index] = column / length;
        half[index] = length * 0.5;
    }

    let centre = model.w_axis.truncate();
    let offset = point - centre;
    // In the box's own frame, where distances are still studs because the
    // axes are unit length — the scale lives in `half` instead.
    let local = Vec3::new(
        offset.dot(axes[0]),
        offset.dot(axes[1]),
        offset.dot(axes[2]),
    );

    let clamped = local.clamp(-half, half);
    // Outside on at least one axis, the nearest surface point is just the
    // clamp; inside, it has to be pushed out through the closest face.
    let outside = clamped != local;
    let face = if outside {
        (0..3)
            .filter(|&axis| clamped[axis] != local[axis])
            .max_by(|&a, &b| (local[a].abs() - half[a]).total_cmp(&(local[b].abs() - half[b])))?
    } else {
        (0..3).min_by(|&a, &b| (half[a] - local[a].abs()).total_cmp(&(half[b] - local[b].abs())))?
    };

    let mut surface = clamped;
    surface[face] = half[face].copysign(sign_of(local[face]));
    // Edges and corners: another face already within reach takes the point
    // with it, so long as the pull doesn't carry it out of reach of where the
    // cursor actually is. A point in the middle of a face one stud wide is
    // within reach of both its edges and belongs on neither.
    for axis in (0..3).filter(|&axis| axis != face) {
        if half[axis] - surface[axis].abs() > reach {
            continue;
        }
        let mut pulled = surface;
        pulled[axis] = half[axis].copysign(sign_of(surface[axis]));
        // The axes are unit length, so a distance measured in the box's frame
        // is already the world-space one.
        if (pulled - local).length() <= reach {
            surface = pulled;
        }
    }

    Some(Surface {
        point: centre + axes[0] * surface.x + axes[1] * surface.y + axes[2] * surface.z,
        normal: axes[face] * sign_of(surface[face]),
    })
}

/// Which way to push a coordinate that sits exactly on the centre line, where
/// `signum` alone would answer with a sign the caller never chose.
fn sign_of(value: f32) -> f32 {
    if value == 0.0 {
        1.0
    } else {
        value.signum()
    }
}

#[cfg(test)]
#[path = "snap/tests.rs"]
mod tests;
