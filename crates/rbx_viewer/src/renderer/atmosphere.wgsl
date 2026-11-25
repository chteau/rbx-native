// Classic fog and Roblox's `Atmosphere`, split out of lighting.wgsl to keep that
// file under the 400-line limit (see `renderer::pipeline`, which concatenates
// this snippet wherever it concatenates lighting.wgsl). Reads the same
// `LightingUniform` and calls `sky_flat`, both declared there, WGSL compiles
// every concatenated snippet as one module, so the split is for readability
// only and changes nothing about how the shader resolves names.

// Optical depth per stud per unit Density, split across RGB channels
// (770/1200/1980 stud half-distances in blue/green/red at Density 1) to match
// Roblox's blue-shifted distance haze. Empirical: Roblox publishes no formula.
// Quartered from the initial fit across three captures, and still a trade-off,
// a Density 0.2 canyon wants no haze at 200 studs while a Density 0.3 plate
// wants more at 300, which no linear-in-Density exponential can give both.
// The scale itself, 0.0013 as tuned, is `lighting.calibration.x`, which the
// user can recalibrate (Studio Settings › Viewport › Advanced).
const ATMOSPHERE_SCATTER: vec3<f32> = vec3<f32>(0.389, 0.641, 1.0);
// Offset is a 0-1 dial in Studio; this is the clear radius it buys at 1.
const ATMOSPHERE_OFFSET_STUDS: f32 = 400.0;

// The last fifth of the render distance is what the fade happens over: short
// enough that most of the view is unaffected, long enough not to read as a wall.
const RENDER_DISTANCE_FADE: f32 = 0.2;

// How much of the haze is `Color` (tinted by `Decay`), the rest being the sky
// it scatters: the real client's horizon under a yellow `Color` stays a pale
// sky blue (the `atmosphere` fidelity scene).
const HAZE_COLOR_WEIGHT: f32 = 0.35;

/// Colour the haze scatters toward, for a ray travelling `direction`.
///
/// `Decay` is a hue rather than a radiance, Studio's haze away from the sun is
/// as bright as `Color`, not as dark as `Decay`, so it is normalized to its own
/// strongest channel before tinting, and fades out over the half of the sky
/// facing the sun.
fn haze_color(direction: vec3<f32>) -> vec3<f32> {
    let decay = lighting.atmosphere_decay.rgb;
    let tint = decay / max(max(decay.r, max(decay.g, decay.b)), 1e-3);
    let toward_sun = dot(direction, normalize(lighting.sun_direction.xyz));
    // Dimmed with the sky itself: the haze is lit by it, so a night horizon
    // fades to dark blue rather than to the daytime white.
    let lit = lighting.atmosphere_color.rgb
        * mix(tint, vec3<f32>(1.0), sqrt(saturate(toward_sun * 0.5 + 0.5)));
    return mix(sky_flat(), lit, HAZE_COLOR_WEIGHT) * lighting.sky_tint.rgb;
}

// The sky is hazed by the same atmosphere, on an air mass that thickens toward
// the horizon: `SKY_DEPTH / (sin(elevation) + SKY_FLOOR)` studs of travel.
// Fitted on the red channel of the same capture, the one Studio does not run
// out of headroom in: its sky is a third of the way to the haze colour two
// degrees up and barely a twentieth at twenty-six, which is a far steeper
// profile than the eye expects, the white band is narrow and the zenith keeps
// the panel's own blue.
const ATMOSPHERE_SKY_DEPTH: f32 = 42.0;
const ATMOSPHERE_SKY_FLOOR: f32 = 0.013;
// `Atmosphere.Haze`, approximated: Roblox thickens the white band along the
// horizon with it, which is what more air on the same view ray does, so it is
// applied as a multiplier on that air mass. Studio's slider stops at 10, where
// this leaves the band some six times deeper and the zenith still blue.
const HAZE_AIR_MASS_GAIN: f32 = 0.5;

/// The sky seen along `direction` through the atmosphere, its panel colour
/// `color`: what the skybox draws, and what the environment probe hands a
/// reflection, so a floor mirroring the horizon at a grazing angle mirrors
/// its haze too (`atmosphere_floor`: the far floor was 5 to 10 levels too
/// blue, reflecting the bare panel's blue).
fn hazed_sky(color: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    let haze = 1.0 + lighting.atmosphere_extra.y * HAZE_AIR_MASS_GAIN;
    let air_mass = ATMOSPHERE_SKY_DEPTH * haze / (max(direction.y, 0.0) + ATMOSPHERE_SKY_FLOOR);
    return mix(haze_color(direction), color, transmittance(air_mass));
}

/// What fraction of a ray survives `studs` of atmosphere, per channel.
fn transmittance(studs: f32) -> vec3<f32> {
    return exp2(-studs * lighting.fog_range.z * lighting.calibration.x * ATMOSPHERE_SCATTER);
}

/// What the far end of the world looks like: the atmosphere's own haze where the
/// place has one, and the flat sky colour where it does not.
fn distant_color(to_eye: vec3<f32>) -> vec3<f32> {
    if lighting.fog_color.w > 0.5 {
        return haze_color(-to_eye);
    }
    return sky_flat() * lighting.sky_tint.rgb;
}

/// How much of a surface `studs` away survives the quality level's own render
/// distance, 1 well inside it and 0 past the end.
///
/// Roblox does not fade anything here, it stops streaming and culls what is too
/// far to draw, so this is a stand-in for that, chosen to be the honest kind:
/// distant geometry sinks into the sky over the last stretch instead of popping
/// out of existence at a hard edge.
fn render_distance_visible(studs: f32) -> f32 {
    let far = lighting.quality.x;
    if far <= 0.0 {
        return 1.0;
    }

    let fade_over = far * RENDER_DISTANCE_FADE;
    return saturate((far - studs) / max(fade_over, 1.0));
}

/// Classic fog is linear in view distance; an Atmosphere replaces it with an
/// exponential falloff whose colour leans toward the sun. The quality level's
/// own render distance then takes whatever either of them left.
fn fade(color: vec3<f32>, to_eye: vec3<f32>, distance: f32) -> vec3<f32> {
    var faded: vec3<f32>;
    if lighting.fog_color.w > 0.5 {
        let clear = ATMOSPHERE_OFFSET_STUDS * lighting.fog_range.w;
        let visible = transmittance(max(distance - clear, 0.0));
        faded = mix(haze_color(-to_eye), color, visible);
    } else {
        let span = max(lighting.fog_range.y - lighting.fog_range.x, 1.0);
        let visible = saturate((lighting.fog_range.y - distance) / span);
        faded = mix(lighting.fog_color.rgb, color, visible);
    }

    return mix(distant_color(to_eye), faded, render_distance_visible(distance));
}
