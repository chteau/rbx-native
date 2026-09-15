//! Builds one `Beam`'s final world-space vertices — the CPU half of the pass;
//! see `pipeline` for the GPU half.

use glam::Vec3;

use super::pipeline::VertexRaw;
use crate::scene::{eval_color, eval_number, Beam, TextureMode};

/// A beam whose two attachments coincide (or whose curve otherwise collapses
/// to a point) would divide by zero building its tangent; that case is
/// already handled in `scene::beam::curve::Curve::tangent`, so nothing here
/// needs its own guard beyond `normalize_or_zero`.
pub(super) fn vertices(beam: &Beam, eye: Vec3, elapsed: f32) -> Vec<VertexRaw> {
    let segments = beam.segments.max(1);
    // Sampled once and reused for both the arc-length estimate (`Wrap` mode)
    // and the per-vertex build below, so the curve is never evaluated twice
    // for the same `t`.
    let centers: Vec<(f32, Vec3)> = (0..=segments)
        .map(|i| {
            let t = i as f32 / segments as f32;
            (t, beam.curve.position(t))
        })
        .collect();

    let repeats = match beam.texture_mode {
        TextureMode::Stretch => beam.texture_length,
        TextureMode::Wrap => arc_length(&centers) / beam.texture_length,
    };

    let mut out = Vec::with_capacity(centers.len() * 2);
    for &(t, center) in &centers {
        let tangent = beam.curve.tangent(t);
        let width_dir = if beam.face_camera {
            let to_camera = (eye - center).normalize_or_zero();
            face_camera_width(tangent, to_camera)
        } else {
            fixed_width(beam.secondary_axis0, beam.secondary_axis1, tangent, t)
        };

        let position = if beam.z_offset != 0.0 {
            center + (eye - center).normalize_or_zero() * beam.z_offset
        } else {
            center
        };
        let half = width_at(beam.width0, beam.width1, t) * 0.5;
        let color = eval_color(&beam.color, t);
        let alpha = (1.0 - eval_number(&beam.transparency, t)).clamp(0.0, 1.0);
        // Positive `TextureSpeed` scrolls the pattern toward Attachment1: a
        // fixed point on the beam samples an *earlier* `v` as time passes,
        // which reads as the texture advancing past it in the other direction.
        //
        // Roblox samples a `Beam`'s texture along its **row** axis (`v`) for
        // the beam's length — confirmed against real `Beam` textures (e.g.
        // asset 5209125673's tick marks are stacked down the image, not
        // across it) — and its column axis (`u`) across the width. A texture
        // authored the other way around renders its repeating pattern
        // squashed into one end of the beam instead of running its length,
        // which is the "rotate my beam texture 90°" fix developers reach for
        // on the DevForum.
        let v = t * repeats - elapsed * beam.texture_speed;

        out.push(VertexRaw {
            position: (position + width_dir * half).to_array(),
            uv: [0.0, v],
            color,
            alpha,
            light_emission: beam.light_emission,
        });
        out.push(VertexRaw {
            position: (position - width_dir * half).to_array(),
            uv: [1.0, v],
            color,
            alpha,
            light_emission: beam.light_emission,
        });
    }
    out
}

/// Appends `next`'s vertices to `into`, a running triangle strip for one
/// texture group, bridging the two strips with two degenerate (zero-area)
/// triangles rather than starting a second draw call. Safe regardless of the
/// parity this inserts into the strip's winding order, since the pipeline
/// culls neither face (see `pipeline::create_pipeline`).
pub(super) fn append(into: &mut Vec<VertexRaw>, next: &[VertexRaw]) {
    if let (Some(&last), Some(&first)) = (into.last(), next.first()) {
        into.push(last);
        into.push(first);
    }
    into.extend_from_slice(next);
}

fn width_at(width0: f32, width1: f32, t: f32) -> f32 {
    width0 + (width1 - width0) * t.clamp(0.0, 1.0)
}

/// Width axis for `FaceCamera = true`: perpendicular to both the curve's
/// tangent and the direction to the eye, so the ribbon plane turns to face
/// the camera as it runs along the beam.
fn face_camera_width(tangent: Vec3, to_camera: Vec3) -> Vec3 {
    let dir = tangent.cross(to_camera);
    if dir.length_squared() > 1e-8 {
        return dir.normalize();
    }
    // Looking straight down the beam: `to_camera` cannot build a basis with
    // `tangent`, so fall back to whichever world axis is least parallel to it.
    let fallback = if tangent.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    tangent.cross(fallback).normalize_or_zero()
}

/// Width axis for `FaceCamera = false`: the attachments' own **Y** axis
/// (`SecondaryAxis`), blended along the curve — see the `Beam` docs' "plane
/// given by the attachments' secondary axes" — then projected perpendicular
/// to the curve's own tangent.
///
/// That projection is the fix: `secondary0`/`secondary1` are each only
/// guaranteed orthogonal to their *own* attachment's `Axis`, never to the
/// chord between the two attachments, so the raw blend can carry a component
/// along `tangent`. Used as-is, that component doesn't widen the ribbon (it
/// stretches it lengthwise instead, invisibly, since only the *magnitude*
/// perpendicular to the strip reads as width) — for a straight beam this
/// projection is a no-op whenever the blend is already perpendicular
/// (the common case: a `SecondaryAxis` deliberately set across the beam's
/// direction), so it changes nothing there; it only ever removes the
/// tangent-parallel slop that a non-perpendicular axis would otherwise leave in.
fn fixed_width(secondary0: Vec3, secondary1: Vec3, tangent: Vec3, t: f32) -> Vec3 {
    let blended = secondary0.lerp(secondary1, t.clamp(0.0, 1.0));
    let projected = (blended - blended.dot(tangent) * tangent).normalize_or_zero();
    if projected != Vec3::ZERO {
        return projected;
    }
    // The blended axis is (nearly) parallel to `tangent` — including the
    // case where `secondary0`/`secondary1` point opposite ways and the raw
    // lerp cancels to zero — so it cannot build a basis with it: same
    // degenerate case `face_camera_width` guards, with the same fallback.
    let fallback = if tangent.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    tangent.cross(fallback).normalize_or_zero()
}

/// Sum of consecutive sample distances — a piecewise-linear approximation of
/// the curve's true length, accurate enough for tiling a texture by since a
/// beam's `Segments` is already the resolution its curve is drawn at.
fn arc_length(centers: &[(f32, Vec3)]) -> f32 {
    centers
        .windows(2)
        .map(|pair| (pair[1].1 - pair[0].1).length())
        .sum()
}

#[cfg(test)]
#[path = "ribbon/tests.rs"]
mod tests;
