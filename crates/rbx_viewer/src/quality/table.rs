//! The per-level bands — the only place to retune what a quality level does.
//!
//! This table is an approximation based on community observation, not official
//! documentation: Roblox publishes no per-level breakdown. The bands are
//! deliberately coarse and in one place to keep parameter changes coordinated.

use super::{QualityLevel, QualityProfile};

/// Bands, each paired with the highest level it covers, in ascending order.
///
/// Eight rows rather than twenty-one: every knob below changes at one of these
/// boundaries, and a row per level would only invite them to drift apart.
const BANDS: [(u8, QualityProfile); 8] = [
    // 1-2: roughly Compatibility — no shadows, no post effects at all.
    (
        2,
        QualityProfile {
            shadows: false,
            shadow_map_size: 1024,
            shadow_pcf_radius_scale: HARD_SHADOW_EDGE,
            shadow_distance: NEAR_SHADOWS_STUDS,
            bloom: false,
            color_correction: false,
            local_lights_max: 0,
            local_shadow_lights_max: 0,
            texture_max_size: 256,
            anisotropy: 1,
            env_reflections: false,
            render_distance: NEAR_RENDER_STUDS,
            decals: true,
            particles: true,
            beams: true,
            trails: true,
            gui: true,
            msaa_samples: 1,
            force_field_intersections: false,
        },
    ),
    // 3-4: the grade comes back, and a handful of local lights with it.
    (
        4,
        QualityProfile {
            shadows: false,
            shadow_map_size: 1024,
            shadow_pcf_radius_scale: HARD_SHADOW_EDGE,
            shadow_distance: NEAR_SHADOWS_STUDS,
            bloom: false,
            color_correction: true,
            local_lights_max: 8,
            local_shadow_lights_max: 0,
            texture_max_size: 512,
            anisotropy: 1,
            env_reflections: false,
            render_distance: MID_RENDER_STUDS,
            decals: true,
            particles: true,
            beams: true,
            trails: true,
            gui: true,
            msaa_samples: 1,
            force_field_intersections: false,
        },
    ),
    // 5-6: shadows and Neon's glow appear — the level users notice.
    (
        6,
        QualityProfile {
            shadows: true,
            shadow_map_size: 1024,
            shadow_pcf_radius_scale: HARD_SHADOW_EDGE,
            shadow_distance: NEAR_SHADOWS_STUDS,
            bloom: true,
            color_correction: true,
            local_lights_max: 64,
            local_shadow_lights_max: 0,
            texture_max_size: 512,
            anisotropy: 4,
            env_reflections: false,
            render_distance: MID_RENDER_STUDS,
            decals: true,
            particles: true,
            beams: true,
            trails: true,
            gui: true,
            msaa_samples: 1,
            force_field_intersections: false,
        },
    ),
    (
        7,
        QualityProfile {
            shadows: true,
            shadow_map_size: 2048,
            shadow_pcf_radius_scale: PLACE_SHADOW_SOFTNESS,
            shadow_distance: NEAR_SHADOWS_STUDS,
            bloom: true,
            color_correction: true,
            local_lights_max: 256,
            local_shadow_lights_max: 4,
            texture_max_size: 1024,
            anisotropy: 4,
            env_reflections: false,
            render_distance: FAR_RENDER_STUDS,
            decals: true,
            particles: true,
            beams: true,
            trails: true,
            gui: true,
            msaa_samples: 1,
            force_field_intersections: false,
        },
    ),
    (
        8,
        QualityProfile {
            shadows: true,
            shadow_map_size: 2048,
            shadow_pcf_radius_scale: PLACE_SHADOW_SOFTNESS,
            shadow_distance: NEAR_SHADOWS_STUDS,
            bloom: true,
            color_correction: true,
            local_lights_max: 256,
            local_shadow_lights_max: 4,
            texture_max_size: 1024,
            anisotropy: 4,
            env_reflections: true,
            render_distance: FAR_RENDER_STUDS,
            decals: true,
            particles: true,
            beams: true,
            trails: true,
            gui: true,
            msaa_samples: 1,
            force_field_intersections: false,
        },
    ),
    (
        9,
        QualityProfile {
            shadows: true,
            shadow_map_size: 2048,
            shadow_pcf_radius_scale: PLACE_SHADOW_SOFTNESS,
            shadow_distance: MID_SHADOWS_STUDS,
            bloom: true,
            color_correction: true,
            local_lights_max: 256,
            local_shadow_lights_max: 4,
            texture_max_size: 1024,
            anisotropy: 4,
            env_reflections: true,
            render_distance: FAR_RENDER_STUDS,
            decals: true,
            particles: true,
            beams: true,
            trails: true,
            gui: true,
            msaa_samples: 1,
            force_field_intersections: false,
        },
    ),
    // 10-15: nothing is capped any more; only the distances still grow.
    (
        15,
        QualityProfile {
            shadows: true,
            shadow_map_size: 4096,
            shadow_pcf_radius_scale: PLACE_SHADOW_SOFTNESS,
            shadow_distance: MID_SHADOWS_STUDS,
            bloom: true,
            color_correction: true,
            local_lights_max: usize::MAX,
            local_shadow_lights_max: 8,
            texture_max_size: 1024,
            anisotropy: 16,
            env_reflections: true,
            render_distance: DISTANT_RENDER_STUDS,
            decals: true,
            particles: true,
            beams: true,
            trails: true,
            gui: true,
            msaa_samples: 1,
            force_field_intersections: false,
        },
    ),
    // 16-21: the levels a desktop client actually runs at, and where
    // `FAR_SHADOWS_STUDS` (see its own doc comment) lands — so the map
    // doubles again here too, for the same reason `7-8` doubles it over
    // `5-6`: leaving the resolution flat while the reach doubles would make
    // `Q21` no sharper than `Q9`'s already-coarser-than-`Q7-8` map.
    (
        QualityLevel::MAX,
        QualityProfile {
            shadows: true,
            shadow_map_size: 8192,
            shadow_pcf_radius_scale: PLACE_SHADOW_SOFTNESS,
            shadow_distance: FAR_SHADOWS_STUDS,
            bloom: true,
            color_correction: true,
            local_lights_max: usize::MAX,
            local_shadow_lights_max: 16,
            texture_max_size: 1024,
            anisotropy: 16,
            env_reflections: true,
            render_distance: f32::INFINITY,
            decals: true,
            particles: true,
            beams: true,
            trails: true,
            gui: true,
            msaa_samples: 4,
            force_field_intersections: true,
        },
    ),
];

/// The highest `texture_max_size` any band above asks for.
///
/// `renderer::texture` uploads every image capped to this, since a level's own
/// cap (`QualityProfile::texture_max_size`) only ever moves the *view* down the
/// mip chain (see `texture::skipped`) — uploading a texel no band can ever view
/// would just be VRAM nothing reads. Bump this if a band above ever needs more.
pub(crate) const MAX_TEXTURE_SIZE: u32 = 1024;

/// View distances, in studs. Roblox streams and culls instead of fading, so
/// these are a stand-in for that (see `render_distance_visible` in
/// lighting.wgsl); the top band never fades at all, which is what keeps a
/// reference capture identical to one taken before this table existed.
pub(super) const NEAR_RENDER_STUDS: f32 = 500.0;
pub(super) const MID_RENDER_STUDS: f32 = 1000.0;
pub(super) const FAR_RENDER_STUDS: f32 = 2000.0;
pub(super) const DISTANT_RENDER_STUDS: f32 = 5000.0;

/// A single PCF tap: the hard shadow edge the lower levels show, whatever the
/// place's own `ShadowSoftness` asks for.
pub(super) const HARD_SHADOW_EDGE: f32 = 0.0;
/// The place's own `ShadowSoftness`, unscaled.
pub(super) const PLACE_SHADOW_SOFTNESS: f32 = 1.0;

/// Shadow reach at the level they first appear at. Half of [`MID_SHADOWS_STUDS`]
/// buys twice the texel density out of the same map, which is the only lever a
/// cascade-free shadow map has.
pub(super) const NEAR_SHADOWS_STUDS: f32 = 150.0;
/// What this renderer used before the table existed: 300 studs still reaches
/// the far end of typical test scenes' shadow maps.
pub(super) const MID_SHADOWS_STUDS: f32 = 300.0;
/// Twice that again. A single non-cascaded shadow map (see `renderer::shadow`)
/// has exactly one lever to trade against reach: its own resolution — the map
/// covers the camera frustum out to this distance no matter how far that is,
/// so doubling the distance alone would halve the texel density every surface
/// in view actually gets, however square the map's own texels are. (The
/// "studs per texel" figure itself isn't `2 * distance / shadow_map_size`
/// either: the frustum sphere `fit()` sizes the map to is wider than it is
/// tall at anything but a 1:1 aspect ratio and a square FOV, so the real
/// figure depends on both.) The top band's `shadow_map_size` is doubled
/// alongside this distance for exactly that reason — see its own comment.
pub(super) const FAR_SHADOWS_STUDS: f32 = 600.0;

/// The band `level` falls in. Levels above the last band get it too, which is
/// what makes the table total without a catch-all row.
pub(super) fn profile(level: u8) -> QualityProfile {
    // The last row covers `QualityLevel::MAX` and callers clamp to it, so the
    // fallback exists only to keep this function total.
    let (_, top) = BANDS[BANDS.len() - 1];
    BANDS
        .iter()
        .find(|(upto, _)| level <= *upto)
        .map_or(top, |(_, profile)| *profile)
}
