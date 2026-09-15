//! The single uniform every pass in `renderer::post` reads: the seven `vec4`s
//! `post.wgsl` declares `PostUniform` as, and how one frame's effects fold into
//! them.
//!
//! Lives apart from `renderer::post` only to keep that file inside the
//! workspace's 400-line guideline; [`Post::raw`] is written against `Post`'s own
//! private state and belongs to it.

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use super::Post;
use crate::camera::NEAR_PLANE;
use crate::lighting::Tonemap;

/// The seven `vec4`s `PostUniform` declares, in order.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct PostRaw {
    bloom: [f32; 4],
    correction: [f32; 4],
    tint: [f32; 4],
    misc: [f32; 4],
    /// x/y: the sun's screen-space UV this frame, z: `SunRaysEffect.Intensity`
    /// (0 disables the whole effect, whether that is because the place has
    /// none, the moon is lit instead, or the projection fell off-screen — see
    /// `renderer::sun::sun_screen_position`), w: `Spread`.
    sun_rays: [f32; 4],
    /// `DepthOfFieldEffect`, in studs and 0-1 mixes: x: `FocusDistance`,
    /// y: `InFocusRadius`, z: `NearIntensity`, w: `FarIntensity`. Whether any of
    /// it applies at all is `misc.w`, not a zero in here — a place may legally
    /// enable the effect with both intensities at zero.
    depth_of_field: [f32; 4],
    /// x: 1 on an orthographic frame, 0 on a perspective one — which
    /// `view_distance` formula the depth buffer needs (see `camera.rs`'s
    /// `reversed_depth`/`orthographic_reversed_depth`). y: the orthographic
    /// far plane in studs, meaningless when x is 0. z/w unused.
    camera: [f32; 4],
}

impl Post {
    /// This frame's uniform: the place's own effects, minus whatever the quality
    /// level has turned off, plus the two things only the frame knows — the tent
    /// radius the bloom chain was sized for, and where the sun landed on screen.
    ///
    /// `sun_screen` is `None` on a frame where the rays must not draw at all (see
    /// [`Post::prepare`]), which zeroes the whole `sun_rays` block.
    pub(super) fn raw(
        &self,
        tent: f32,
        sun_screen: Option<Vec2>,
        orthographic_far: Option<f32>,
    ) -> PostRaw {
        let correction = self
            .effects
            .color_correction
            .filter(|_| self.color_correction);
        // Both halves have to be present at once: a screen position with no
        // effect (or vice versa) means nothing to the shader.
        let sun_rays = self.effects.sun_rays.zip(sun_screen);
        let depth_of_field = self.effects.depth_of_field;

        PostRaw {
            bloom: [self.intensity(), self.effects.bloom.threshold, tent, 0.0],
            correction: [
                correction.map_or(0.0, |grade| grade.brightness),
                correction.map_or(0.0, |grade| grade.contrast),
                correction.map_or(0.0, |grade| grade.saturation),
                f32::from(u8::from(correction.is_some())),
            ],
            tint: correction.map_or([1.0; 4], |grade| {
                [grade.tint.x, grade.tint.y, grade.tint.z, 0.0]
            }),
            misc: [
                f32::from(u8::from(self.effects.tonemap == Tonemap::Retro)),
                f32::from(u8::from(self.effects.blur.is_some())),
                // The one number the depth reconstruction needs: reversed-Z with
                // an infinite far plane puts a pixel `d` studs out at depth
                // `NEAR_PLANE / d` and nothing else enters into it (see
                // `camera`), so the shader needs no matrix of its own.
                NEAR_PLANE,
                f32::from(u8::from(depth_of_field.is_some())),
            ],
            sun_rays: sun_rays.map_or([0.0; 4], |(rays, uv)| {
                [uv.x, uv.y, rays.intensity, rays.spread]
            }),
            depth_of_field: depth_of_field.map_or([0.0; 4], |dof| {
                [
                    dof.focus_distance,
                    dof.in_focus_radius,
                    dof.near_intensity,
                    dof.far_intensity,
                ]
            }),
            camera: [
                f32::from(u8::from(orthographic_far.is_some())),
                orthographic_far.unwrap_or(0.0),
                0.0,
                0.0,
            ],
        }
    }
}
