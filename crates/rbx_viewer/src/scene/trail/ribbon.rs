//! Samples one `Trail`'s appearance (`WidthScale`/`Color`/`Transparency`)
//! along its recorded [`super::history::Recorder`] — the eye-independent half
//! of building a trail's ribbon; `renderer::trail::ribbon` does the rest
//! (`FaceCamera`, the final GPU vertices) once it knows where the camera is.

use glam::Vec3;

use super::history::{Recorder, Sample};
use super::instance::Trail;
use crate::scene::{eval_color, eval_number};

/// One point along a trail's ribbon: still just the two attachment positions
/// it was recorded at, plus everything downstream needs to turn it into a
/// quad edge without touching a `NumberSequence`/`ColorSequence` again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RibbonPoint {
    pub(crate) position0: Vec3,
    pub(crate) position1: Vec3,
    /// `WidthScale` at this point's age, already evaluated — multiplies the
    /// distance between `position0` and `position1` (see `Trail.WidthScale`'s
    /// docs' worked example).
    pub(crate) width_scale: f32,
    pub(crate) color: [f32; 3],
    pub(crate) alpha: f32,
    /// How far along the trail's length this point sits, in studs travelled
    /// by its midpoint — what `TextureLength` tiles against, the same
    /// arc-length role `renderer::beam::ribbon::arc_length` fills for a beam.
    pub(crate) distance: f32,
}

/// Turns a recorder's history into ribbon points, oldest first — consecutive
/// points already read as a triangle strip in the order returned.
///
/// A trail needs two samples to have any shape at all; fewer than that (this
/// viewer's own case whenever nothing has moved — see `scene::trail`'s module
/// doc) yields an empty result rather than a degenerate one-point strip.
pub(crate) fn segments(trail: &Trail, recorder: &Recorder, now: f32) -> Vec<RibbonPoint> {
    let samples = recorder.samples();
    if samples.len() < 2 {
        return Vec::new();
    }

    let mut distance = 0.0;
    let mut previous_mid: Option<Vec3> = None;
    samples
        .iter()
        .map(|sample| {
            let mid = (sample.position0 + sample.position1) * 0.5;
            if let Some(previous) = previous_mid {
                distance += (mid - previous).length();
            }
            previous_mid = Some(mid);

            let t = age_t(trail, sample, now);
            RibbonPoint {
                position0: sample.position0,
                position1: sample.position1,
                width_scale: eval_number(&trail.width_scale, t),
                color: eval_color(&trail.color, t),
                alpha: (1.0 - eval_number(&trail.transparency, t)).clamp(0.0, 1.0),
                distance,
            }
        })
        .collect()
}

/// Where `sample` falls in `WidthScale`/`Color`/`Transparency`'s own `0..1`
/// domain: **0 is the newest end** (age 0, right at the live attachments) and
/// **1 is the oldest, about-to-expire end** (age `Lifetime`).
///
/// Confirmed by `Trail.Lifetime`'s own docs: "the lifetime of a trail is also
/// used by that effect's `Color` and `Transparency` properties to determine
/// how each segment is drawn ... as the segment ages" — age runs from 0 at
/// creation up to `Lifetime` at expiry, and each sequence is defined over
/// that same `0..1` span in the same direction.
fn age_t(trail: &Trail, sample: &Sample, now: f32) -> f32 {
    ((now - sample.time) / trail.lifetime).clamp(0.0, 1.0)
}

#[cfg(test)]
#[path = "ribbon/tests.rs"]
mod tests;
