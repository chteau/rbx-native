//! The renderer's half of an isolated view (see `Headless::set_isolation`):
//! which of the place's surroundings a frame leaves out, what it is lit with
//! instead, and the backdrop it is drawn on. Which *parts* draw is the
//! scene's half — see `scene::isolation`.
//!
//! Pure decisions, kept apart from [`super::Renderer::draw`] so they can be
//! tested without a device.

use super::envmap::{Probe, FACES};
use crate::lighting::Lighting;
use crate::quality::QualityProfile;
use crate::scene::srgb_to_linear;

/// The flat backdrop an isolated view is drawn on, as encoded sRGB:
/// Blender's own viewport grey, `#3A3A3A`. Dark enough that no rig colour
/// washes out against it and light enough that a black one still reads. It
/// lands on screen as exactly this value: the resolve's default tone map is
/// a plain clamp, and the backdrop is far below every bloom threshold.
pub const ISOLATION_BACKDROP: [u8; 3] = [0x3A, 0x3A, 0x3A];

/// The sky irradiance an isolated view's ambient is rebuilt from, linear, at
/// every one of the six cardinal normals alike — a flat grey dome standing in
/// for the place's own sky, so a blue noon or a red sunset cannot tint the
/// rig. About what a clear Studio sky gives a grey part's shaded side,
/// without the blue.
const NEUTRAL_SKY: f32 = 0.22;

/// Which of the place's own surroundings a frame draws besides its parts.
///
/// Every field is a pass [`super::Renderer::draw`] keeps in step with the
/// place either way — an edit to the sky, the terrain or an emitter lands
/// while it is skipped — so turning one back on draws exactly what a frame
/// that never skipped it would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Surroundings {
    /// The skybox, the stars and the sun and moon discs.
    pub(super) sky: bool,
    /// Terrain, its water and grass, and the shadows it casts.
    pub(super) terrain: bool,
    /// Beams, trails, particles and the `BillboardGui`/`SurfaceGui`
    /// canvases. On in an isolated frame too: the scene hands the renderer
    /// only the isolated rig's own effects then (see
    /// `Scene::drawn_particle_emitters`), so a sword's trail or a torch's
    /// fire still plays on the stage while the rest of the place's do not.
    pub(super) effects: bool,
    /// The place's own `SelectionBox`/`*HandleAdornment` family, whose
    /// adornee an isolation may well have hidden.
    pub(super) adornments: bool,
    /// The `ScreenGui` overlay.
    pub(super) screen_gui: bool,
    /// The place's `PointLight`s, `SpotLight`s and `SurfaceLight`s, and their
    /// shadow maps.
    pub(super) local_lights: bool,
}

impl Surroundings {
    /// Everything for a normal frame; for an isolated one, only the rig's
    /// own effects (which the scene has already narrowed to) on its
    /// backdrop.
    pub(super) fn of(isolated: bool) -> Self {
        let drawn = !isolated;
        Surroundings {
            sky: drawn,
            terrain: drawn,
            effects: true,
            adornments: drawn,
            screen_gui: drawn,
            local_lights: drawn,
        }
    }
}

/// The colour the scene pass starts from: `place`, the fallback behind a
/// place's sky, or the isolation's backdrop, linearised for the HDR target
/// (the `*Srgb` display target encodes it back on the way out).
pub(super) fn clear_color(isolated: bool, place: wgpu::Color) -> wgpu::Color {
    if !isolated {
        return place;
    }
    let [r, g, b] =
        ISOLATION_BACKDROP.map(|channel| f64::from(srgb_to_linear(f32::from(channel) / 255.0)));
    wgpu::Color { r, g, b, a: 1.0 }
}

/// The lighting, probe and quality profile a frame is lit with: the place's
/// own for a normal frame, and for an isolated one [`Lighting::neutral`], a
/// probe whose irradiance is [`NEUTRAL_SKY`] all round, and reflections that
/// read that flat grey rather than the place's sky cube — the one input of
/// the probe the lighting uniform cannot replace, since the cube itself is
/// bound, not copied.
pub(super) fn lit(
    isolated: bool,
    lighting: &Lighting,
    probe: Probe,
    quality: QualityProfile,
) -> (Lighting, Probe, QualityProfile) {
    if !isolated {
        return (*lighting, probe, quality);
    }
    let probe = Probe {
        top_mip: probe.top_mip,
        irradiance: [[NEUTRAL_SKY, NEUTRAL_SKY, NEUTRAL_SKY, 0.0]; FACES],
    };
    let quality = QualityProfile {
        env_reflections: false,
        ..quality
    };
    (Lighting::neutral(), probe, quality)
}

#[cfg(test)]
#[path = "isolation/tests.rs"]
mod tests;
