//! The lighting uniform: [`crate::lighting::Lighting`] plus the moving camera,
//! packed the way `lighting.wgsl` reads it.

mod local;

pub(super) use local::{buffer as local_lights_buffer, write as local_lights_write};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use super::envmap::Probe;
use super::shadow::{Fit, Lamp};
use crate::lighting::{Clouds, Fog, Lighting};
use crate::quality::QualityProfile;

/// How much of the sky's own irradiance `EnvironmentDiffuseScale = 1` is worth.
///
/// Calibrated against real Studio renders: the integral unscaled lands grey
/// surfaces on blue-grey values matching the sky's daylight irradiance.
/// Below 1 turns shadows back to neutral grey, which signals this term is wrong.
const ENV_DIFFUSE_WEIGHT: f32 = 1.0;

/// PCF kernel radius, in shadow-map texels, at `ShadowSoftness = 1`.
///
/// Studio's default 0.2 has to land on a penumbra of a couple of texels — the
/// reference capture's cubes have an edge roughly a stud wide — which is where
/// a fifth of this comes out right; 0 is then a single tap, i.e. the hard edge
/// Roblox gives it, and 1 a penumbra several studs across.
const SOFTNESS_TEXELS: f32 = 8.0;

/// Constant slice off the receiver's own depth, in studs, on top of the
/// caster-side slope bias. Small enough that no shadow visibly leaves the foot
/// of what casts it, large enough to swallow the last of the depth rounding.
const DEPTH_BIAS_STUDS: f32 = 0.12;

/// The `vec4`s of `LightingUniform`, in the order it declares them. All-`vec4` on
/// purpose: a uniform buffer pads every `vec3` up to 16 bytes anyway, so
/// spelling the padding out is what keeps the two declarations impossible to
/// drift apart.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct LightingRaw {
    sun_direction: [f32; 4],
    sun_color: [f32; 4],
    fill_color: [f32; 4],
    ambient: [f32; 4],
    fog_color: [f32; 4],
    fog_range: [f32; 4],
    atmosphere_color: [f32; 4],
    atmosphere_decay: [f32; 4],
    camera: [f32; 4],
    tuning: [f32; 4],
    /// xyz: what the sky and the light it bounces are multiplied by at this
    /// time of day. w: how much of the star field shows.
    sky_tint: [f32; 4],
    /// The two `Atmosphere` dials that are neither colour nor density.
    atmosphere_extra: [f32; 4],
    /// Sky irradiance at the six cardinal normals, in cube map layer order.
    sky_irradiance: [[f32; 4]; 6],
    /// World to shadow-map clip space, for whichever lamp is above the horizon.
    light_view_projection: [[f32; 4]; 4],
    shadow_params: [f32; 4],
    shadow_lamp: [f32; 4],
    /// x: how many entries of the local light buffer the shader has to loop
    /// over. The buffer itself is never empty (see [`local`]), so the count is
    /// the only thing that tells an unlit scene apart from a lit one.
    locals: [f32; 4],
    /// The two quality knobs the shading itself reads. x: render distance in
    /// studs, 0 meaning unlimited — an infinity would poison every arithmetic it
    /// touched, so it is flattened to the one value no fade can start at.
    /// y: 1 where the environment terms sample the probe, 0 where they fall back
    /// to one flat sky colour.
    quality: [f32; 4],
    /// `Clouds.Color`, linear like everything else here; w is unused.
    clouds_color: [f32; 4],
    /// x: `Clouds.Cover`, 0 drawing nothing — which is also what no enabled
    /// `Clouds` instance collapses to (see `crate::lighting::clouds`).
    /// y: `Clouds.Density`.
    clouds_extra: [f32; 4],
}

impl LightingRaw {
    pub(super) const SIZE: wgpu::BufferAddress = std::mem::size_of::<Self>() as _;

    pub(super) fn new(
        lighting: &Lighting,
        camera: Vec3,
        probe: &Probe,
        shadow: (Lamp, &Fit),
        lights: usize,
        quality: &QualityProfile,
    ) -> Self {
        let (fog_color, fog_range, atmosphere_color, atmosphere_decay) = fog(&lighting.fog);
        let (clouds_color, clouds_extra) = clouds(lighting.clouds);
        let (lamp, fit) = shadow;
        let (glare, haze) = match lighting.fog {
            Fog::Atmosphere { glare, haze, .. } => (glare, haze),
            Fog::Linear { .. } => (0.0, 0.0),
        };

        LightingRaw {
            sun_direction: vec4(lighting.sun_direction, 0.0),
            sun_color: vec4(lighting.sun_color, 0.0),
            fill_color: vec4(lighting.fill_color, 0.0),
            ambient: vec4(lighting.ambient, 0.0),
            fog_color,
            fog_range,
            atmosphere_color,
            atmosphere_decay,
            camera: vec4(camera, 0.0),
            tuning: [
                lighting.exposure,
                lighting.environment_specular,
                lighting.environment_diffuse * ENV_DIFFUSE_WEIGHT,
                probe.top_mip,
            ],
            sky_tint: vec4(lighting.sky_tint, lighting.star_fade),
            atmosphere_extra: [glare, haze, 0.0, 0.0],
            sky_irradiance: probe.irradiance,
            light_view_projection: fit.view_projection.to_cols_array_2d(),
            shadow_params: [
                lighting.shadow_softness.clamp(0.0, 1.0)
                    * SOFTNESS_TEXELS
                    * quality.shadow_pcf_radius_scale,
                fit.texel_uv,
                fit.texel_studs,
                DEPTH_BIAS_STUDS / fit.depth_studs.max(1.0),
            ],
            shadow_lamp: [lamp.marker(), 0.0, 0.0, 0.0],
            locals: [lights as f32, 0.0, 0.0, 0.0],
            quality: [
                if quality.render_distance.is_finite() {
                    quality.render_distance
                } else {
                    0.0
                },
                f32::from(u8::from(quality.env_reflections)),
                0.0,
                0.0,
            ],
            clouds_color: vec4(clouds_color, 0.0),
            clouds_extra,
        }
    }
}

/// Both fog flavours share the same four slots; the marker in `fog_color.w`
/// picks which of them the shader reads.
fn fog(fog: &Fog) -> ([f32; 4], [f32; 4], [f32; 4], [f32; 4]) {
    match *fog {
        Fog::Linear { color, start, end } => {
            (vec4(color, 0.0), [start, end, 0.0, 0.0], [0.0; 4], [0.0; 4])
        }
        Fog::Atmosphere {
            color,
            decay,
            density,
            offset,
            ..
        } => (
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, density, offset],
            vec4(color, 0.0),
            vec4(decay, 0.0),
        ),
    }
}

fn vec4(value: Vec3, w: f32) -> [f32; 4] {
    [value.x, value.y, value.z, w]
}

/// No enabled `Clouds` layer packs to `Cover = 0`, which the sky shader already
/// has to treat as "draw nothing" for the property itself — so there is no
/// separate presence marker here, unlike `fog_color.w` above.
fn clouds(clouds: Option<Clouds>) -> (Vec3, [f32; 4]) {
    let Some(clouds) = clouds else {
        return (Vec3::ZERO, [0.0; 4]);
    };
    (clouds.color, [clouds.cover, clouds.density, 0.0, 0.0])
}

#[cfg(test)]
#[path = "lighting/tests.rs"]
mod tests;
