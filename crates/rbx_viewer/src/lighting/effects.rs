//! The post-processing effects a place hangs off `Lighting`, as the resolve
//! pass reads them: a `BloomEffect`, a `BlurEffect`, a `ColorCorrectionEffect`,
//! a `ColorGradingEffect`, a `SunRaysEffect` and a `DepthOfFieldEffect`.
//!
//! Entry point: [`read`]. Everything here is CPU-side; the GPU packing lives in
//! `renderer::post`.

use glam::Vec3;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{boolean, number};

const BLOOM_CLASS: &str = "BloomEffect";
const BLUR_CLASS: &str = "BlurEffect";
const COLOR_CORRECTION_CLASS: &str = "ColorCorrectionEffect";
const COLOR_GRADING_CLASS: &str = "ColorGradingEffect";
const SUN_RAYS_CLASS: &str = "SunRaysEffect";
const DEPTH_OF_FIELD_CLASS: &str = "DepthOfFieldEffect";

/// `ColorGradingEffect.TonemapperPreset` as Roblox's enum numbers it.
const TONEMAPPER_PRESET_RETRO: u32 = 1;

// Roblox's property defaults, which is what an effect that leaves one out gets.
// Note that a *fresh Studio place* does not ship these: its own BloomEffect is
// written out as Intensity 1, Size 24, Threshold 2.
const DEFAULT_BLOOM_INTENSITY: f32 = 0.4;
const DEFAULT_BLOOM_SIZE: f32 = 24.0;
const DEFAULT_BLOOM_THRESHOLD: f32 = 0.95;
/// `BlurEffect.Size`, Roblox's own default — the same unit (pixels at 1080p)
/// as `BloomEffect.Size`, which is why both share `renderer::post::bloom`'s
/// resolution-aware radius function.
const DEFAULT_BLUR_SIZE: f32 = 24.0;
/// Every fixture this renderer has ever seen a `SunRaysEffect` in serializes
/// both `Intensity` and `Spread` explicitly (all at 0.01/0.1, deliberately
/// subtle), so there is no known-good capture of Studio's own fresh default to
/// fit against. Zero is the safe fallback either way: an instance that somehow
/// omits `Intensity` renders no rays at all rather than guessed ones, exactly
/// how a garbled `TonemapperPreset` below falls back to `Default` instead of
/// being half-trusted.
const DEFAULT_SUN_RAYS_INTENSITY: f32 = 0.0;
const DEFAULT_SUN_RAYS_SPREAD: f32 = 0.0;
// `DepthOfFieldEffect` defaults are verified against real Studio places:
// multiple examples with untouched effects all serialize exactly these four
// numbers — and all disabled, which matches Studio's own default.
const DEFAULT_FOCUS_DISTANCE: f32 = 0.05;
const DEFAULT_IN_FOCUS_RADIUS: f32 = 30.0;
const DEFAULT_NEAR_INTENSITY: f32 = 0.75;
const DEFAULT_FAR_INTENSITY: f32 = 0.1;

/// How far past the sharp zone's own edge the blur ramps in over, as a multiple
/// of `InFocusRadius`.
///
/// Roblox publishes no falloff whatsoever — only the four properties — so this
/// is a choice, and the choice is to reuse the one length the effect already
/// carries rather than to invent a constant in studs: a place that widens its
/// sharp zone widens the gradient either side of it in the same proportion,
/// which is what keeps the effect in scale with places built at wildly
/// different sizes. Mirrored by `DOF_FALLOFF_RADII` in `post.wgsl`, which is the
/// copy that actually runs; this one only exists for [`DepthOfField::blur_factor`].
#[cfg(test)]
const DOF_FALLOFF_RADII: f32 = 1.0;
/// Floor on the falloff length, in studs: `InFocusRadius = 0` is then a hard
/// step at the focus plane rather than a division by zero.
#[cfg(test)]
const DOF_MIN_FALLOFF: f32 = 1.0e-3;

/// Rec. 709 luma, which is what the saturation term rotates around.
#[cfg(test)]
const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// Contrast pivots here rather than around black, so raising it darkens the
/// lower half of the range and brightens the upper half instead of just
/// brightening everything.
#[cfg(test)]
const CONTRAST_PIVOT: f32 = 0.5;

/// `BloomEffect`: which pixels glow, and how far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bloom {
    pub(crate) intensity: f32,
    /// Blur width in pixels, quoted at 1080p (see `renderer::post::bloom`).
    pub(crate) size: f32,
    /// Linear radiance a pixel has to clear before any of it spreads.
    pub(crate) threshold: f32,
}

/// `BlurEffect`: a full-frame blur that replaces the sharp image outright,
/// which is what Roblox itself does — there is no partial-strength mix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Blur {
    /// Radius in pixels, quoted at 1080p — same convention as [`Bloom::size`],
    /// and scaled by the same function in `renderer::post::bloom`.
    pub(crate) size: f32,
}

/// `ColorCorrectionEffect`, which grades the frame after the bloom is in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ColorCorrection {
    pub(crate) brightness: f32,
    pub(crate) contrast: f32,
    pub(crate) saturation: f32,
    /// A per-channel gain, not a radiance: `TintColor` is used as the raw 0-1
    /// components Studio shows, without the sRGB linearization every colour
    /// that stands for light goes through.
    pub(crate) tint: Vec3,
}

/// `SunRaysEffect`: an occlusion-masked radial blur toward the sun's projected
/// screen position, which is what actually casts the rays — see
/// `renderer::sun::sun_screen_position` for where that projection comes from
/// and `renderer::post`'s resolve shader for the taps themselves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SunRays {
    pub(crate) intensity: f32,
    /// How far the taps reach along the pixel-to-sun line, as a fraction of
    /// that line's own length — see the resolve shader for the exact mapping.
    pub(crate) spread: f32,
}

/// `ColorGradingEffect.TonemapperPreset`: how the final HDR colour is folded
/// into the displayable range.
///
/// Roblox publishes no formula for either curve — only that `Retro` "emulates
/// the pre-2019 Roblox lighting system" — so both are principled
/// approximations, not a reproduction. `Default` is exactly this renderer's
/// existing tonemap (a plain clamp; see `renderer::post`'s resolve shader), so
/// a place with no `ColorGradingEffect`, or one that resolves to `Default`,
/// looks pixel-identical to before this enum existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tonemap {
    #[default]
    Default,
    Retro,
}

/// `DepthOfFieldEffect`: where the sharp zone is, and how blurred each side of
/// it gets.
///
/// All four numbers are Roblox's own properties; what the renderer does with
/// them (one blurred copy of the frame, mixed in per pixel by the reconstructed
/// depth) is `renderer::post`'s business, and the ramp between the two is
/// [`DepthOfField::blur_factor`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DepthOfField {
    /// Centre of the sharp zone, in studs down the view axis. Studio's own
    /// default is a degenerate 0.05 — closer than the near plane — which is
    /// taken literally: a place that means something else says so.
    pub(crate) focus_distance: f32,
    /// Distance in studs the sharp zone extends on EACH side of
    /// [`DepthOfField::focus_distance`] — despite the name, this is not a
    /// diameter: Roblox's own docs for `InFocusRadius` are explicit that it is
    /// "the distance away from FocusDistance (on both sides)", so the sharp
    /// zone is `2 * in_focus_radius` wide, not `in_focus_radius` wide.
    pub(crate) in_focus_radius: f32,
    /// How much of the blurred frame the nearer-than-focus side ramps to, 0 to 1.
    pub(crate) near_intensity: f32,
    /// The same for the farther-than-focus side, which Roblox lets a place set
    /// independently of the near one.
    pub(crate) far_intensity: f32,
}

/// Everything the resolve pass applies over the finished frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Effects {
    pub(crate) bloom: Bloom,
    /// `None` where the place has no `BlurEffect`, or every one of them is
    /// disabled.
    pub(crate) blur: Option<Blur>,
    /// `None` where the place has no `ColorCorrectionEffect`, or every one of
    /// them is disabled.
    pub(crate) color_correction: Option<ColorCorrection>,
    /// `ColorGradingEffect::default()` (i.e. [`Tonemap::Default`]) where the
    /// place has none, or every one of them is disabled or absent — which
    /// carries no behavioural change on its own.
    pub(crate) tonemap: Tonemap,
    /// `None` where the place has no `SunRaysEffect`, or every one of them is
    /// disabled. This alone does not decide whether the rays actually draw
    /// this frame — see [`SunRays`]'s own doc comment.
    pub(crate) sun_rays: Option<SunRays>,
    /// `None` where the place has no `DepthOfFieldEffect`, or every one of them
    /// is disabled — Studio ships the effect turned off by default.
    pub(crate) depth_of_field: Option<DepthOfField>,
}

impl Default for Bloom {
    fn default() -> Self {
        Bloom {
            intensity: DEFAULT_BLOOM_INTENSITY,
            size: DEFAULT_BLOOM_SIZE,
            threshold: DEFAULT_BLOOM_THRESHOLD,
        }
    }
}

/// The blur ramp in Rust, for the same reason [`ColorCorrection::apply`] exists:
/// `post.wgsl`'s `dof_blur_factor` is the copy that runs, and a ramp that drifts
/// between the two puts the sharp zone somewhere other than where the place asked
/// for it.
#[cfg(test)]
impl DepthOfField {
    /// How much of the blurred frame a pixel `distance` studs down the view axis
    /// takes: 0 inside the sharp zone, ramping to `NearIntensity` or
    /// `FarIntensity` depending on which side of the focus it falls.
    fn blur_factor(&self, distance: f32) -> f32 {
        let offset = distance - self.focus_distance;
        let beyond = (offset.abs() - self.in_focus_radius).max(0.0);
        let falloff = (self.in_focus_radius * DOF_FALLOFF_RADII).max(DOF_MIN_FALLOFF);
        let intensity = if offset < 0.0 {
            self.near_intensity
        } else {
            self.far_intensity
        };

        (beyond / falloff).min(1.0) * intensity
    }
}

/// The grade in Rust, which is the only form of it a test can call: the frame
/// itself is graded by `post.wgsl`'s `fs_resolve`, and the two are the same
/// arithmetic written twice on purpose — a shader cannot be unit-tested, and an
/// order of operations that drifts between them is invisible until a place
/// looks wrong.
#[cfg(test)]
impl ColorCorrection {
    /// The grade: tint, then brightness, then contrast, then saturation, on a
    /// tone-mapped, gamma-encoded colour (the shader grades the displayable
    /// frame, not linear light).
    ///
    /// Roblox's docs describe each property's own effect independently and
    /// never state a combination order, so this chain is this renderer's own
    /// choice, not a verified one; each property's formula, on the other hand,
    /// is: Brightness -1/1 map to fully black/white, Contrast pivots on grey,
    /// Saturation -1 is full desaturation and >1 keeps intensifying — all
    /// matched exactly to create.roblox.com/docs, ColorCorrectionEffect.
    fn apply(&self, color: Vec3) -> Vec3 {
        let tinted = color * self.tint + Vec3::splat(self.brightness);
        let contrasted = (tinted - Vec3::splat(CONTRAST_PIVOT)) * (1.0 + self.contrast)
            + Vec3::splat(CONTRAST_PIVOT);

        Vec3::splat(contrasted.dot(LUMA)).lerp(contrasted, 1.0 + self.saturation)
    }
}

/// Reads the effects parented to the `Lighting` service.
///
/// Only the first *enabled* effect of each class counts: Roblox composites them
/// all, but a place that carries two of a kind almost always has one turned off
/// as a spare. `BlurEffect` is the one exception — see [`strongest_blur`] for
/// the tie-break Roblox documents for it specifically.
pub(super) fn read(dom: &WeakDom, database: &ReflectionDatabase, lighting: Ref) -> Effects {
    Effects {
        bloom: enabled(dom, lighting, |instance| {
            database.is_subclass_of(instance.class(), BLOOM_CLASS)
        })
        .map(bloom)
        .unwrap_or_default(),
        // Unlike every other effect here, Roblox documents BlurEffect's
        // multi-instance rule explicitly: "Only one BlurEffect can be applied
        // at once (the instance with the greatest Size takes priority)" —
        // create.roblox.com/docs, BlurEffect.Size. Not "first enabled".
        blur: strongest_blur(dom, database, lighting).map(blur),
        color_correction: enabled(dom, lighting, |instance| {
            database.is_subclass_of(instance.class(), COLOR_CORRECTION_CLASS)
        })
        .map(color_correction),
        // `ColorGradingEffect` is missing from this renderer's embedded API dump
        // (see `assets/API-Dump.json`) even though Roblox documents it, so
        // `is_subclass_of` can never confirm it — an exact name match is the
        // only test available, which is fine since nothing subclasses it.
        tonemap: enabled(dom, lighting, |instance| {
            instance.class() == COLOR_GRADING_CLASS
        })
        .map(tonemap)
        .unwrap_or_default(),
        sun_rays: enabled(dom, lighting, |instance| {
            database.is_subclass_of(instance.class(), SUN_RAYS_CLASS)
        })
        .map(sun_rays),
        depth_of_field: enabled(dom, lighting, |instance| {
            database.is_subclass_of(instance.class(), DEPTH_OF_FIELD_CLASS)
        })
        .map(depth_of_field),
    }
}

/// The properties of the first enabled child matching `is_a_match`.
///
/// `Enabled` missing means enabled: it defaults to true, and a place that never
/// touched it does not serialize it.
fn enabled(
    dom: &WeakDom,
    lighting: Ref,
    is_a_match: impl Fn(&rbx_dom::Instance) -> bool,
) -> Option<&std::collections::BTreeMap<String, Variant>> {
    let children = dom.get(lighting)?.children();

    children
        .iter()
        .filter_map(|&child| dom.get(child))
        .filter(|instance| is_a_match(instance))
        .map(rbx_dom::Instance::properties)
        .find(|properties| boolean(properties.get("Enabled")).unwrap_or(true))
}

/// The properties of the *enabled* `BlurEffect` child with the greatest
/// effective `Size` — Roblox's own tie-break for having more than one, unlike
/// every other effect [`enabled`] serves. Sizes are compared after the same
/// missing-property/negative-value handling [`blur`] applies, so a `BlurEffect`
/// that never touched `Size` competes at Roblox's own default rather than at 0.
///
/// Ties keep whichever instance this happens to visit last (`Iterator::max_by`);
/// Roblox does not document a tie-break, so any deterministic choice is as
/// faithful as any other.
fn strongest_blur<'a>(
    dom: &'a WeakDom,
    database: &ReflectionDatabase,
    lighting: Ref,
) -> Option<&'a std::collections::BTreeMap<String, Variant>> {
    let children = dom.get(lighting)?.children();

    children
        .iter()
        .filter_map(|&child| dom.get(child))
        .filter(|instance| database.is_subclass_of(instance.class(), BLUR_CLASS))
        .map(rbx_dom::Instance::properties)
        .filter(|properties| boolean(properties.get("Enabled")).unwrap_or(true))
        .max_by(|a, b| blur_size(a).total_cmp(&blur_size(b)))
}

fn bloom(properties: &std::collections::BTreeMap<String, Variant>) -> Bloom {
    let fallback = Bloom::default();

    Bloom {
        intensity: number(properties.get("Intensity"))
            .unwrap_or(fallback.intensity)
            .max(0.0),
        size: number(properties.get("Size"))
            .unwrap_or(fallback.size)
            .max(0.0),
        // Negative thresholds are legal in Studio and mean "everything glows".
        threshold: number(properties.get("Threshold")).unwrap_or(fallback.threshold),
    }
}

fn blur(properties: &std::collections::BTreeMap<String, Variant>) -> Blur {
    Blur {
        size: blur_size(properties),
    }
}

/// A negative `Size` is not physically meaningful, so it floors at 0 the same
/// way [`bloom`]'s does — shared by [`blur`] and [`strongest_blur`], which both
/// need the exact same number: the second must compare the sizes it will
/// ultimately hand to the first.
fn blur_size(properties: &std::collections::BTreeMap<String, Variant>) -> f32 {
    number(properties.get("Size"))
        .unwrap_or(DEFAULT_BLUR_SIZE)
        .max(0.0)
}

fn color_correction(properties: &std::collections::BTreeMap<String, Variant>) -> ColorCorrection {
    ColorCorrection {
        brightness: number(properties.get("Brightness")).unwrap_or(0.0),
        contrast: number(properties.get("Contrast")).unwrap_or(0.0),
        saturation: number(properties.get("Saturation")).unwrap_or(0.0),
        tint: gain(properties.get("TintColor")).unwrap_or(Vec3::ONE),
    }
}

/// Any value but `Retro` — including no `TonemapperPreset` at all, which is
/// what a place that never touched it serializes — defaults to `Default`.
fn tonemap(properties: &std::collections::BTreeMap<String, Variant>) -> Tonemap {
    match properties.get("TonemapperPreset") {
        Some(&Variant::Enum(TONEMAPPER_PRESET_RETRO)) => Tonemap::Retro,
        _ => Tonemap::Default,
    }
}

fn sun_rays(properties: &std::collections::BTreeMap<String, Variant>) -> SunRays {
    SunRays {
        intensity: number(properties.get("Intensity"))
            .unwrap_or(DEFAULT_SUN_RAYS_INTENSITY)
            .max(0.0),
        spread: number(properties.get("Spread"))
            .unwrap_or(DEFAULT_SUN_RAYS_SPREAD)
            .max(0.0),
    }
}

fn depth_of_field(properties: &std::collections::BTreeMap<String, Variant>) -> DepthOfField {
    DepthOfField {
        focus_distance: number(properties.get("FocusDistance"))
            .unwrap_or(DEFAULT_FOCUS_DISTANCE)
            .max(0.0),
        in_focus_radius: number(properties.get("InFocusRadius"))
            .unwrap_or(DEFAULT_IN_FOCUS_RADIUS)
            .max(0.0),
        // Studio's own sliders stop at 1, and a mix above it would read as
        // "blurrier than the blurred copy", which is not a thing this pass has.
        near_intensity: number(properties.get("NearIntensity"))
            .unwrap_or(DEFAULT_NEAR_INTENSITY)
            .clamp(0.0, 1.0),
        far_intensity: number(properties.get("FarIntensity"))
            .unwrap_or(DEFAULT_FAR_INTENSITY)
            .clamp(0.0, 1.0),
    }
}

fn gain(value: Option<&Variant>) -> Option<Vec3> {
    match value? {
        Variant::Color3(color) => Some(Vec3::new(color.r, color.g, color.b)),
        &Variant::Color3uint8 { r, g, b } => Some(Vec3::new(r.into(), g.into(), b.into()) / 255.0),
        _ => None,
    }
}

#[cfg(test)]
#[path = "effects/tests.rs"]
mod tests;
