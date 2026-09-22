//! Roblox's `Lighting` service reduced to what the shaders need: two lamp
//! colours, an ambient term, the sun's direction and whatever fades the
//! distance (classic fog, or an `Atmosphere` child if the place has one).
//!
//! Entry point: [`Lighting::from_dom`]. Everything here is CPU-side; the GPU
//! packing lives in `renderer::lighting`.

mod clouds;
mod effects;
mod local;
mod sky;
pub mod sun;

pub(crate) use clouds::Clouds;
pub(crate) use effects::{Effects, Tonemap};
pub use local::light_guides;
pub(crate) use local::{local_lights, LocalLight};
use sun::sun_direction;

use glam::Vec3;
use rbx_dom::{Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::scene::{descendants, srgb_to_linear};

const LIGHTING_CLASS: &str = "Lighting";
const ATMOSPHERE_CLASS: &str = "Atmosphere";

// Defaults are the values Studio writes into a fresh baseplate, so a place that
// omits one lands where Studio would have put it rather than at zero.
const DEFAULT_AMBIENT: f32 = 70.0 / 255.0;
const DEFAULT_BRIGHTNESS: f32 = 3.0;
const DEFAULT_CLOCK_HOURS: f32 = 14.5;
const DEFAULT_FOG_GREY: f32 = 192.0 / 255.0;
const DEFAULT_FOG_END: f32 = 100_000.0;
const DEFAULT_ENVIRONMENT_SCALE: f32 = 1.0;
// `Lighting.ShadowSoftness` as Studio writes it into a fresh place; `GlobalShadows`
// is on by default there too, and a place that predates the property still gets
// shadows under `Technology = ShadowMap`.
const DEFAULT_SHADOW_SOFTNESS: f32 = 0.2;

/// Radiance of the sun lamp at `Brightness = 1`, before `ColorShift_Top`.
///
/// Calibrated against a real Studio render with Brightness 3, OutdoorAmbient
/// 70/255, sun 46 degrees up at 14:30, EnvironmentDiffuseScale 1. The value
/// 0.45 matches measured grey face outputs to Studio when tone mapping
/// differences are accounted for.
const SUN_BASE: f32 = 0.45;
/// The second lamp, opposite the sun: what keeps a shaded face from reading as a
/// flat silhouette. Roblox feeds it from its own constant buffer; a tenth of the
/// sun, tinted cool, is what lands the shaded side of that same 163-grey part
/// between Studio's (94, 110, 137) and (108, 122, 147) once the sky irradiance
/// is in.
const FILL_FRACTION: f32 = 0.10;
const FILL_TINT: Vec3 = Vec3::new(0.82, 0.9, 1.0);
/// Moonlight, once the sun is below the horizon: same lamp, a quarter as bright
/// and much colder.
const MOON_FRACTION: f32 = 0.25;
const MOON_TINT: Vec3 = Vec3::new(0.55, 0.68, 1.0);
/// Half-width, in `sun_direction.y`, of the band the sun hands the fill lamp
/// over to the moon across. Without it dawn and dusk would pop in one frame.
const TWILIGHT_BAND: f32 = 0.1;

/// How the distance is faded out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Fog {
    /// Classic Roblox fog: linear in view distance between `start` and `end`.
    Linear { color: Vec3, start: f32, end: f32 },
    /// An `Atmosphere` child, which replaces the fog entirely.
    Atmosphere {
        color: Vec3,
        decay: Vec3,
        density: f32,
        offset: f32,
        /// `Atmosphere.Glare`: how much the sun's halo burns through the sky.
        glare: f32,
        /// `Atmosphere.Haze`: how thick the band along the horizon gets.
        haze: f32,
    },
}

/// Everything the surface shaders read, with the place's sRGB colours already
/// linearized and its two lamps already mixed for the time of day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Lighting {
    /// Unit vector pointing *at* the sun, i.e. `L` in `dot(N, L)`. Stays the
    /// sun's own direction at night, where the fill lamp at `-L` is the moon.
    pub(crate) sun_direction: Vec3,
    pub(crate) sun_color: Vec3,
    pub(crate) fill_color: Vec3,
    pub(crate) ambient: Vec3,
    pub(crate) exposure: f32,
    pub(crate) environment_diffuse: f32,
    pub(crate) environment_specular: f32,
    /// `Lighting.GlobalShadows`. False skips the shadow map entirely, which is
    /// how a place asks for the flat, everything-lit look.
    pub(crate) global_shadows: bool,
    /// `Lighting.ShadowSoftness`, 0 (hard edge) to 1 (very soft). Drives the
    /// PCF kernel radius, nothing else.
    pub(crate) shadow_softness: f32,
    pub(crate) fog: Fog,
    /// What the sky, the light it bounces and its haze are all multiplied by at
    /// this time of day: white by day, a dim blue at night (see [`sky`]).
    pub(crate) sky_tint: Vec3,
    /// How much of the star field shows, 0 by day to 1 at night.
    pub(crate) star_fade: f32,
    pub(crate) effects: Effects,
    /// The place's `Clouds` layer, if `Terrain` carries an enabled one.
    pub(crate) clouds: Option<Clouds>,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting::assemble(&Defaults::default(), None, Effects::default(), None)
    }
}

/// The raw properties, before the lamps are mixed — kept apart so the mixing is
/// one function with one set of inputs whether they came from a file or not.
struct Defaults {
    ambient: Vec3,
    brightness: f32,
    shift_top: Vec3,
    shift_bottom: Vec3,
    clock: f32,
    latitude: f32,
    exposure: f32,
    environment_diffuse: f32,
    environment_specular: f32,
    global_shadows: bool,
    shadow_softness: f32,
    fog_color: Vec3,
    fog_start: f32,
    fog_end: f32,
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            ambient: Vec3::splat(srgb_to_linear(DEFAULT_AMBIENT)),
            brightness: DEFAULT_BRIGHTNESS,
            shift_top: Vec3::ZERO,
            shift_bottom: Vec3::ZERO,
            clock: DEFAULT_CLOCK_HOURS,
            latitude: 0.0,
            exposure: 0.0,
            environment_diffuse: DEFAULT_ENVIRONMENT_SCALE,
            environment_specular: DEFAULT_ENVIRONMENT_SCALE,
            global_shadows: true,
            shadow_softness: DEFAULT_SHADOW_SOFTNESS,
            fog_color: Vec3::splat(srgb_to_linear(DEFAULT_FOG_GREY)),
            fog_start: 0.0,
            fog_end: DEFAULT_FOG_END,
        }
    }
}

impl Lighting {
    /// Reads the `Lighting` service, falling back to Studio's own defaults for
    /// anything the file leaves out — including the service itself, which a bare
    /// `.rbxm` model never has.
    ///
    /// `clock_override` is `--clock-time`, which beats both `ClockTime` and
    /// `TimeOfDay` so a screenshot can pick its own hour.
    pub(crate) fn from_dom(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        clock_override: Option<f32>,
    ) -> Self {
        // Independent of whether a `Lighting` service is even found below:
        // `Clouds` renders off `Terrain`, not off `Lighting` (see `clouds`).
        let clouds = clouds::read(dom, database);

        let Some(referent) = descendants(dom).find(|&referent| {
            dom.get(referent)
                .is_some_and(|instance| database.is_subclass_of(instance.class(), LIGHTING_CLASS))
        }) else {
            return Lighting::from_clock(clock_override, clouds);
        };
        let Some(instance) = dom.get(referent) else {
            return Lighting::from_clock(clock_override, clouds);
        };

        let properties = instance.properties();
        let fallback = Defaults::default();
        let read = Defaults {
            // "The effective OutdoorAmbient value is clamped to be greater
            // than or equal to Ambient in all channels" — Lighting.OutdoorAmbient
            // docs. A place that lifts Ambient above it lifts the outdoors too.
            ambient: match (
                color(properties.get("OutdoorAmbient")),
                color(properties.get("Ambient")),
            ) {
                (Some(outdoor), Some(indoor)) => outdoor.max(indoor),
                (outdoor, indoor) => outdoor.or(indoor).unwrap_or(fallback.ambient),
            },
            brightness: number(properties.get("Brightness")).unwrap_or(fallback.brightness),
            shift_top: color(properties.get("ColorShift_Top")).unwrap_or(Vec3::ZERO),
            shift_bottom: color(properties.get("ColorShift_Bottom")).unwrap_or(Vec3::ZERO),
            clock: clock_override.unwrap_or_else(|| clock_of(properties)),
            latitude: number(properties.get("GeographicLatitude")).unwrap_or(0.0),
            exposure: number(properties.get("ExposureCompensation")).unwrap_or(0.0),
            environment_diffuse: number(properties.get("EnvironmentDiffuseScale"))
                .unwrap_or(fallback.environment_diffuse),
            environment_specular: number(properties.get("EnvironmentSpecularScale"))
                .unwrap_or(fallback.environment_specular),
            global_shadows: boolean(properties.get("GlobalShadows"))
                .unwrap_or(fallback.global_shadows),
            shadow_softness: number(properties.get("ShadowSoftness"))
                .unwrap_or(fallback.shadow_softness),
            fog_color: color(properties.get("FogColor")).unwrap_or(fallback.fog_color),
            fog_start: number(properties.get("FogStart")).unwrap_or(fallback.fog_start),
            fog_end: number(properties.get("FogEnd")).unwrap_or(fallback.fog_end),
        };

        Lighting::assemble(
            &read,
            atmosphere(dom, database, referent),
            effects::read(dom, database, referent),
            clouds,
        )
    }

    fn from_clock(clock_override: Option<f32>, clouds: Option<Clouds>) -> Self {
        let mut defaults = Defaults::default();
        if let Some(clock) = clock_override {
            defaults.clock = clock;
        }
        Lighting::assemble(&defaults, None, Effects::default(), clouds)
    }

    /// Mixes the two lamps for the time of day, handing the fill lamp over from
    /// a cool bounce to moonlight as the sun crosses the horizon.
    fn assemble(
        read: &Defaults,
        atmosphere: Option<Fog>,
        effects: Effects,
        clouds: Option<Clouds>,
    ) -> Self {
        let sun_direction = sun_direction(read.clock, read.latitude);
        // 1 in full daylight, 0 once the sun is a hair below the horizon.
        let day = ((sun_direction.y + TWILIGHT_BAND) / (2.0 * TWILIGHT_BAND)).clamp(0.0, 1.0);
        let lamp = SUN_BASE * read.brightness.max(0.0);

        Lighting {
            sun_direction,
            sun_color: Vec3::splat(lamp * day) * (Vec3::ONE + read.shift_top),
            // The fill lamp sits at -L, which is exactly where the moon is once
            // the sun has set: one lamp does both jobs, and nothing has to swap
            // direction mid-twilight.
            fill_color: Vec3::splat(lamp)
                * (FILL_FRACTION * FILL_TINT * day + MOON_FRACTION * MOON_TINT * (1.0 - day))
                * (Vec3::ONE + read.shift_bottom),
            ambient: read.ambient,
            exposure: read.exposure.exp2(),
            environment_diffuse: read.environment_diffuse.max(0.0),
            environment_specular: read.environment_specular.max(0.0),
            global_shadows: read.global_shadows,
            shadow_softness: read.shadow_softness.clamp(0.0, 1.0),
            fog: atmosphere.unwrap_or(Fog::Linear {
                color: read.fog_color,
                start: read.fog_start,
                end: read.fog_end.max(read.fog_start + 1.0),
            }),
            sky_tint: sky::tint(sun_direction.y),
            star_fade: sky::star_fade(sun_direction.y),
            effects,
            clouds,
        }
    }
}

/// `ClockTime` when the file carries it, otherwise the `TimeOfDay` string
/// Studio actually serializes ("14:30:00").
fn clock_of(properties: &std::collections::BTreeMap<String, Variant>) -> f32 {
    if let Some(hours) = number(properties.get("ClockTime")) {
        return hours;
    }
    properties
        .get("TimeOfDay")
        .and_then(text)
        .and_then(parse_clock)
        .unwrap_or(DEFAULT_CLOCK_HOURS)
}

/// "HH", "HH:MM" or "HH:MM:SS" as fractional hours. Anything else is refused
/// rather than half-parsed: a garbled time is better replaced by Studio's
/// default than by an hour nobody wrote.
fn parse_clock(text: &str) -> Option<f32> {
    let mut hours = 0.0;
    let mut scale = 1.0;
    let mut fields = 0;

    for field in text.split(':') {
        let value: f32 = field.trim().parse().ok()?;
        if !value.is_finite() {
            return None;
        }
        hours += value * scale;
        scale /= 60.0;
        fields += 1;
    }

    (1..=3).contains(&fields).then_some(hours)
}

/// An `Atmosphere` child replaces the classic fog outright, which is what
/// Roblox does too — the two never composite.
fn atmosphere(dom: &WeakDom, database: &ReflectionDatabase, lighting: rbx_dom::Ref) -> Option<Fog> {
    let children = dom.get(lighting)?.children();
    let referent = *children.iter().find(|&&child| {
        dom.get(child)
            .is_some_and(|instance| database.is_subclass_of(instance.class(), ATMOSPHERE_CLASS))
    })?;
    let properties = dom.get(referent)?.properties();

    Some(Fog::Atmosphere {
        color: color(properties.get("Color")).unwrap_or(Vec3::splat(srgb_to_linear(0.78))),
        decay: color(properties.get("Decay")).unwrap_or(Vec3::splat(srgb_to_linear(0.45))),
        density: number(properties.get("Density")).unwrap_or(0.3).max(0.0),
        offset: number(properties.get("Offset")).unwrap_or(0.0).max(0.0),
        // Both reach the sky shader rather than a screen-space pass: see
        // `lighting.wgsl`, where each is an approximation of what the property
        // visibly does rather than of how Roblox computes it.
        glare: number(properties.get("Glare")).unwrap_or(0.0).max(0.0),
        haze: number(properties.get("Haze")).unwrap_or(0.0).max(0.0),
    })
}

// Colours in a place file are sRGB, and every one of them is multiplied into
// linear radiance here, so they are linearized on the way in.
pub(super) fn color(value: Option<&Variant>) -> Option<Vec3> {
    match value? {
        Variant::Color3(color) => Some(Vec3::from([color.r, color.g, color.b].map(srgb_to_linear))),
        &Variant::Color3uint8 { r, g, b } => Some(Vec3::from(
            [r, g, b].map(|channel| srgb_to_linear(f32::from(channel) / 255.0)),
        )),
        _ => None,
    }
}

pub(super) fn boolean(value: Option<&Variant>) -> Option<bool> {
    match value? {
        Variant::Bool(flag) => Some(*flag),
        _ => None,
    }
}

pub(super) fn number(value: Option<&Variant>) -> Option<f32> {
    let value = match value? {
        Variant::Float32(value) => *value,
        Variant::Float64(value) => *value as f32,
        Variant::Int32(value) => *value as f32,
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn text(value: &Variant) -> Option<&str> {
    match value {
        Variant::String(text) => Some(text),
        _ => None,
    }
}

#[cfg(test)]
#[path = "lighting/tests.rs"]
mod tests;
