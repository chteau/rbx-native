//! Turns one `Trail`'s ribbon points (`crate::scene::trail_segments`) into
//! final world-space vertices — the GPU-facing half of the pass; see
//! `crate::scene::trail` for the eye-independent half (segment recording,
//! sequence sampling by age).
//!
//! `FaceCamera` is implicit for every trail (see `scene::trail::Trail`'s
//! doc), so every point's width axis is built the same way
//! `renderer::beam::ribbon::face_camera_width` builds a `Beam`'s one —
//! duplicated here rather than reused since that function is `pub(super)`,
//! scoped to `renderer::beam` only.

use glam::Vec3;

use super::pipeline::VertexRaw;
use crate::scene::{trail_segments, Trail, TrailRecorder};

/// Builds one trail's ribbon vertices from its recorded history.
///
/// Fewer than two ribbon points (this viewer's own case whenever nothing has
/// moved — see `scene::trail`'s module doc) returns an empty vector: there is
/// no segment to draw, so nothing is appended to the shared vertex buffer and
/// no draw call ever names this trail.
pub(super) fn vertices(
    trail: &Trail,
    recorder: &TrailRecorder,
    eye: Vec3,
    now: f32,
) -> Vec<VertexRaw> {
    let points = trail_segments(trail, recorder, now);
    if points.len() < 2 {
        return Vec::new();
    }

    let midpoints: Vec<Vec3> = points
        .iter()
        .map(|point| (point.position0 + point.position1) * 0.5)
        .collect();

    let mut out = Vec::with_capacity(points.len() * 2);
    for (i, point) in points.iter().enumerate() {
        let mid = midpoints[i];
        let tangent = travel_tangent(&midpoints, i);
        let to_camera = (eye - mid).normalize_or_zero();
        let width_dir = face_camera_width(tangent, to_camera);

        let half_width = (point.position1 - point.position0).length() * point.width_scale * 0.5;
        let u = point.distance / trail.texture_length;

        out.push(VertexRaw {
            position: (mid + width_dir * half_width).to_array(),
            uv: [u, 0.0],
            color: point.color,
            alpha: point.alpha,
            light_emission: trail.light_emission,
        });
        out.push(VertexRaw {
            position: (mid - width_dir * half_width).to_array(),
            uv: [u, 1.0],
            color: point.color,
            alpha: point.alpha,
            light_emission: trail.light_emission,
        });
    }
    out
}

/// Appends `next`'s vertices to `into`, bridging two trails sharing one
/// texture group with two degenerate triangles rather than a second draw
/// call — identical trick to `renderer::beam::ribbon::append`.
pub(super) fn append(into: &mut Vec<VertexRaw>, next: &[VertexRaw]) {
    if let (Some(&last), Some(&first)) = (into.last(), next.first()) {
        into.push(last);
        into.push(first);
    }
    into.extend_from_slice(next);
}

/// Unit direction of travel along the trail's midpoint path at point `i`, a
/// central difference where both neighbours exist and a one-sided one at
/// either end.
fn travel_tangent(midpoints: &[Vec3], i: usize) -> Vec3 {
    let before = midpoints[i.saturating_sub(1)];
    let after = midpoints[(i + 1).min(midpoints.len() - 1)];
    let direction = (after - before).normalize_or_zero();
    if direction != Vec3::ZERO {
        return direction;
    }
    // Every recorded position along the trail coincides (only possible with
    // `MinLength` at 0 and no real movement): no direction of travel is
    // defined, so pick a stable arbitrary one rather than NaN — matching
    // `scene::beam::curve::Curve::tangent`'s identical fallback.
    Vec3::Z
}

/// Width axis: perpendicular to both the direction of travel and the eye, so
/// the ribbon always turns to face the camera — identical formula to
/// `renderer::beam::ribbon::face_camera_width`.
fn face_camera_width(tangent: Vec3, to_camera: Vec3) -> Vec3 {
    let dir = tangent.cross(to_camera);
    if dir.length_squared() > 1e-8 {
        return dir.normalize();
    }
    let fallback = if tangent.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    tangent.cross(fallback).normalize_or_zero()
}

#[cfg(test)]
#[path = "ribbon/tests.rs"]
mod tests;
