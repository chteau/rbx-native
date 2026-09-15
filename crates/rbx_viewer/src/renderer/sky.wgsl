// The sky's own shading: the haze along the horizon and the halo round the sun.
//
// Split out of lighting.wgsl and concatenated only in front of skybox.wgsl (see
// `renderer::pipeline`), which is the one pass that shades a panel at infinity:
// a surface shader has no use for either function, and the whole model was
// outgrowing one file.

// The sky is hazed by the same atmosphere, on an air mass that thickens toward
// the horizon: `SKY_DEPTH / (sin(elevation) + SKY_FLOOR)` studs of travel.
// Fitted on the red channel of the same capture, the one Studio does not run
// out of headroom in: its sky is a third of the way to the haze colour two
// degrees up and barely a twentieth at twenty-six, which is a far steeper
// profile than the eye expects — the white band is narrow and the zenith keeps
// the panel's own blue.
const ATMOSPHERE_SKY_DEPTH: f32 = 42.0;
const ATMOSPHERE_SKY_FLOOR: f32 = 0.013;
// `Atmosphere.Haze`, approximated: Roblox thickens the white band along the
// horizon with it, which is what more air on the same view ray does, so it is
// applied as a multiplier on that air mass. Studio's slider stops at 10, where
// this leaves the band some six times deeper and the zenith still blue.
const HAZE_AIR_MASS_GAIN: f32 = 0.5;
// `Atmosphere.Glare`, approximated: the property brightens and widens the halo
// around the sun, so it is one power lobe about the light direction, tightest
// at Glare 0 and broad at Studio's own maximum. Nothing here is Roblox's own
// formula — the engine computes its glare in screen space over the finished
// frame, which this pass has no access to.
const GLARE_TIGHT_EXPONENT: f32 = 200.0;
const GLARE_WIDE_EXPONENT: f32 = 8.0;
const GLARE_STRENGTH: f32 = 0.6;
const GLARE_FULL: f32 = 10.0;
// The halo follows the sun under the horizon rather than blinking out with it.
const GLARE_HORIZON_FADE: f32 = 8.0;

// `Clouds`: a flat deck at a fixed height above the eye, read through the view
// direction the way an infinite ground plane is — `direction.xz / direction.y`
// is where that ray crosses a plane one unit up, so scaling it by a height in
// studs gives a 2D coordinate on the deck without ever touching a texture.
// Chosen purely by eye: big enough that a full sky sweep shows several cloud
// shapes, small enough that the coordinate stays a few noise cells wide near
// the zenith, where `direction.y` is largest and the plane coordinate is at
// its smallest.
const CLOUD_LAYER_HEIGHT: f32 = 60.0;
// Floor on `direction.y` before it becomes a divisor: without it the plane
// coordinate diverges approaching the horizon, long before the horizon fade
// below has a chance to hide the result.
const CLOUD_MIN_ELEVATION: f32 = 0.06;
// How many deck units one noise cell spans. Fitted so the deck's own height
// above puts several cells across the visible sky rather than one enormous
// blob or an undifferentiated speckle.
const CLOUD_NOISE_SCALE: f32 = 0.01;
// Half-width of the smoothstep band around the `noise > 1 - Cover` edge: a
// hard compare would draw a jagged silhouette, since the field is only C0
// continuous at the octave boundaries.
const CLOUD_EDGE_SOFTNESS: f32 = 0.12;
// `Clouds.Density`, approximated: Roblox's own docs describe it as "mainly
// affecting transparency", heavy at 1 and "light, semi-translucent" toward 0 —
// so it drives most of the blend weight, with this floor keeping even the
// thinnest cloud somewhat visible rather than vanishing outright.
const CLOUD_MIN_OPACITY: f32 = 0.35;
// The same property also reads as "heavy, dark clouds with a stormy
// appearance" at 1, which this renderer takes literally: `Density = 1` darkens
// the lit colour by this fraction on top of the opacity above.
const CLOUD_DENSITY_DARKENING: f32 = 0.45;
// The band of `direction.y` the deck fades out across, so the plane
// projection's own blow-up near the horizon is hidden by design rather than by
// the luck of the noise field landing low there.
const CLOUD_HORIZON_FADE_LOW: f32 = 0.03;
const CLOUD_HORIZON_FADE_HIGH: f32 = 0.22;

// The classic one-liner hash (Card/'iq'): cheap, no textures, and good enough
// for shapes this low-frequency. Its only job is to decorrelate the four
// corners `value_noise` interpolates between.
fn cloud_hash(cell: vec2<f32>) -> f32 {
    return fract(sin(dot(cell, vec2<f32>(127.1, 311.7))) * 43758.5453123);
}

/// Bilinear value noise: one hashed value per integer cell corner, eased by a
/// Hermite curve so the field has a continuous derivative and no visible grid.
fn value_noise(point: vec2<f32>) -> f32 {
    let cell = floor(point);
    let local = fract(point);

    let bottom = mix(cloud_hash(cell), cloud_hash(cell + vec2<f32>(1.0, 0.0)), local.x);
    let top = mix(
        cloud_hash(cell + vec2<f32>(0.0, 1.0)),
        cloud_hash(cell + vec2<f32>(1.0, 1.0)),
        local.x,
    );
    let eased = local * local * (3.0 - 2.0 * local);
    return mix(bottom, top, eased.y);
}

/// Four octaves of the value noise above, normalized back to `[0, 1]`: one
/// coarse pass for the cloud deck's own shape, three finer ones so its edges
/// are not a smooth blob.
fn cloud_field(point: vec2<f32>) -> f32 {
    var total = 0.0;
    var normalization = 0.0;
    var amplitude = 1.0;
    var frequency = 1.0;
    for (var octave = 0; octave < 4; octave++) {
        total += value_noise(point * frequency) * amplitude;
        normalization += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    return total / normalization;
}

/// `Clouds`, blended over the sky wherever the noise field clears its own
/// `Cover` threshold. `Cover = 0` (Roblox's own "sparse" end of the slider) is
/// this renderer's stand-in for `Enabled = false` too — see
/// `renderer::lighting`, which packs both to the same zero.
fn clouds(sky: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    let cover = lighting.clouds_extra.x;
    if cover <= 0.0 || direction.y <= 0.0 {
        return sky;
    }

    let elevation = max(direction.y, CLOUD_MIN_ELEVATION);
    let plane = direction.xz * (CLOUD_LAYER_HEIGHT / elevation) * CLOUD_NOISE_SCALE;
    let field = cloud_field(plane);

    let threshold = 1.0 - cover;
    let shape = smoothstep(
        threshold - CLOUD_EDGE_SOFTNESS,
        threshold + CLOUD_EDGE_SOFTNESS,
        field,
    );
    let horizon = smoothstep(CLOUD_HORIZON_FADE_LOW, CLOUD_HORIZON_FADE_HIGH, direction.y);
    let opacity = shape * horizon * mix(CLOUD_MIN_OPACITY, 1.0, lighting.clouds_extra.y);

    // Lit by whichever lamp is actually up, the same pair every surface reads:
    // the sun by day, the fill lamp's moonlight once it has set.
    let cloud_light = max(lighting.sun_color.rgb + lighting.fill_color.rgb, vec3<f32>(0.05));
    let lit = lighting.clouds_color.rgb * cloud_light * (1.0 - lighting.clouds_extra.y * CLOUD_DENSITY_DARKENING);

    return mix(sky, lit, opacity);
}

/// The extra light the sun spills over the sky around it, as `Atmosphere.Glare`
/// asks for. Additive, like the sun's own disc.
fn glare(direction: vec3<f32>) -> vec3<f32> {
    let amount = lighting.atmosphere_extra.x;
    if amount <= 0.0 {
        return vec3<f32>(0.0);
    }

    let light = normalize(lighting.sun_direction.xyz);
    let width = saturate(amount / GLARE_FULL);
    let lobe = pow(saturate(dot(direction, light)), mix(GLARE_TIGHT_EXPONENT, GLARE_WIDE_EXPONENT, width));
    let above = saturate(light.y * GLARE_HORIZON_FADE + 1.0);

    return lighting.atmosphere_color.rgb * (amount * GLARE_STRENGTH * lobe * above);
}

/// The same atmosphere, over the sky itself: a skybox panel is infinitely far
/// away, so its haze is set by the air mass along the view ray rather than by
/// any distance. Studio hazes its sky exactly like this — the horizon of a
/// default Atmosphere is white, not blue.
fn shade_sky(color: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    // One set of panels covers the whole day, so the night comes from here:
    // the same tint the sky's own bounce and haze are dimmed by.
    var lit = color * lighting.sky_tint.rgb;
    // Before the haze below: the deck sits far closer than the "infinite" air
    // mass the atmosphere models, so the same horizon whiteout that dims the
    // panel behind it dims the clouds too, rather than painting over them.
    lit = clouds(lit, direction);
    if lighting.fog_color.w > 0.5 {
        let haze = 1.0 + lighting.atmosphere_extra.y * HAZE_AIR_MASS_GAIN;
        let air_mass = ATMOSPHERE_SKY_DEPTH * haze / (max(direction.y, 0.0) + ATMOSPHERE_SKY_FLOOR);
        lit = mix(haze_color(direction), lit, transmittance(air_mass));
        lit += glare(direction);
    }
    return max(lit * lighting.tuning.x, vec3<f32>(0.0));
}
