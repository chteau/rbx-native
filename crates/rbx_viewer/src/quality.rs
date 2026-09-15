//! Roblox's graphics quality levels, as one table of renderer knobs.
//!
//! `Enum.QualityLevel` is `Automatic` plus `Level01` to `Level21`, and Roblox
//! documents only what rises with it — "rendering distance, shading quality,
//! apparent geometry and texture resolution, limits on particles, fidelity of
//! trail effects, post-effect quality and more" — never which level turns what
//! on. Entry point: [`QualityLevel::profile`], whose bands live in `table`;
//! `Automatic` is [`FrameRateManager`], in `auto`.

mod auto;
mod table;

use std::str::FromStr;

pub use auto::FrameRateManager;
pub(crate) use table::MAX_TEXTURE_SIZE;

/// A graphics quality setting: Roblox's own `Automatic`, or one of its 21
/// discrete levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityLevel {
    Automatic,
    /// A level from [`QualityLevel::MIN`] to [`QualityLevel::MAX`]; anything
    /// outside that is clamped rather than refused, since the enum is public and
    /// nothing downstream can do better with an out-of-range level.
    Level(u8),
}

impl QualityLevel {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 21;

    /// The level a frame is actually drawn at — what a host showing the level on
    /// screen displays.
    ///
    /// `Automatic` answers [`QualityLevel::MAX`]: choosing a level from the
    /// measured frame rate needs a frame clock, which a screenshot has none of,
    /// and a fixture that rendered differently from one run to the next would be
    /// useless as a reference. A host with a clock drives [`FrameRateManager`]
    /// instead and sets the [`QualityLevel::Level`] it reports, so it never
    /// resolves `Automatic` at all.
    pub fn resolved(self) -> u8 {
        match self {
            QualityLevel::Automatic => QualityLevel::MAX,
            QualityLevel::Level(level) => level.clamp(QualityLevel::MIN, QualityLevel::MAX),
        }
    }

    pub(crate) fn profile(self) -> QualityProfile {
        table::profile(self.resolved())
    }
}

impl Default for QualityLevel {
    /// The top level, not `Automatic`: every caller that has a frame clock says
    /// so explicitly, and the ones that do not (screenshots, fixtures) must not
    /// drift.
    fn default() -> Self {
        QualityLevel::Level(QualityLevel::MAX)
    }
}

impl FromStr for QualityLevel {
    type Err = String;

    /// `auto`/`automatic`, a bare `1` to `21`, or Roblox's own `Level07`
    /// spelling — case-insensitively, since the enum item is `Level07` while a
    /// command line is habitually lowercase.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        let lowercase = trimmed.to_ascii_lowercase();
        if lowercase == "auto" || lowercase == "automatic" {
            return Ok(QualityLevel::Automatic);
        }

        let level: u8 = lowercase
            .strip_prefix("level")
            .unwrap_or(&lowercase)
            .parse()
            .map_err(|_| format!("'{trimmed}' is not a quality level"))?;
        if !(QualityLevel::MIN..=QualityLevel::MAX).contains(&level) {
            return Err(format!(
                "quality level {level} is outside {}-{}",
                QualityLevel::MIN,
                QualityLevel::MAX
            ));
        }
        Ok(QualityLevel::Level(level))
    }
}

/// What one level costs the renderer, as the dials it already has.
///
/// Read straight by the renderer; every value in it comes from `table`, which is
/// the only place to retune.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct QualityProfile {
    /// False draws no shadow map at all and samples none, which is the single
    /// biggest visual step in the whole range.
    pub(crate) shadows: bool,
    /// Side of the sun's shadow map, in texels.
    pub(crate) shadow_map_size: u32,
    /// What the place's `ShadowSoftness` is worth here: 0 collapses the PCF
    /// kernel to a single tap, which is the hard-edged shadow of the lower
    /// levels, and 1 is the place's own penumbra.
    pub(crate) shadow_pcf_radius_scale: f32,
    /// How far from the eye the sun's shadow map still reaches, in studs. One
    /// map, no cascades, so this trades directly against its sharpness.
    pub(crate) shadow_distance: f32,
    /// False keeps the post chain but skips the bloom passes, which is what
    /// takes the glow off a Neon part.
    pub(crate) bloom: bool,
    /// False ignores the place's `ColorCorrectionEffect`.
    pub(crate) color_correction: bool,
    /// How many of the place's `PointLight`s, `SpotLight`s and `SurfaceLight`s
    /// are uploaded at all; `usize::MAX` is every one of them.
    pub(crate) local_lights_max: usize,
    /// How many `SpotLight`/`SurfaceLight`s with `Shadows = true` get their own
    /// depth map, at most — one map per light, so this is also the shadow
    /// texture array's own layer count. `PointLight`s never cast one (see
    /// `renderer::shadow::local`), whatever this allows.
    pub(crate) local_shadow_lights_max: usize,
    /// Longest side any decal, `Texture`, sky panel or material pack is uploaded
    /// at, in texels; bigger images are uploaded from a lower mip level.
    pub(crate) texture_max_size: u32,
    /// Anisotropic sample count, 1 being plain trilinear filtering. What keeps a
    /// heavily tiled baseplate readable at a grazing angle.
    pub(crate) anisotropy: u16,
    /// False stops the environment terms sampling the sky probe: a `Reflectance`
    /// part or a metal falls back to one flat sky colour instead of a reflection.
    pub(crate) env_reflections: bool,
    /// How far geometry is still drawn at full strength, in studs; beyond it the
    /// last stretch fades into the sky. `f32::INFINITY` never fades.
    pub(crate) render_distance: f32,
    /// Always true in the table, and a knob anyway: Roblox never drops decals,
    /// but the pass they are drawn in is the obvious one to cut if it ever has
    /// to be.
    pub(crate) decals: bool,
    /// Always true in the table; `--no-particles` is the only thing that turns
    /// it off, by overriding the field after `QualityLevel::profile()` builds
    /// it (see `lib::run`) — no level in Roblox's own table drops particles.
    pub(crate) particles: bool,
    /// Always true in the table; `--no-beams` is the only thing that turns it
    /// off, the same way `--no-particles` overrides `particles` above.
    pub(crate) beams: bool,
    /// Always true in the table; `--no-trails` is the only thing that turns
    /// it off, the same way `--no-beams` overrides `beams` above.
    pub(crate) trails: bool,
    /// Always true in the table; `--no-gui` is the only thing that turns it
    /// off, the same way `--no-trails` overrides `trails` above.
    pub(crate) gui: bool,
    /// Samples every scene pass is drawn at, 1 being no multisampling: what takes
    /// the stair-stepping off a part's silhouette.
    ///
    /// The one knob baked into the pipelines rather than into a uniform or a bind
    /// group, so crossing the level it changes at is the single expensive switch
    /// in the table — see `renderer::switch`. Clamped down where the adapter
    /// cannot multisample the HDR format (`renderer::post`).
    pub(crate) msaa_samples: u32,
}

#[cfg(test)]
#[path = "quality/tests.rs"]
mod tests;
