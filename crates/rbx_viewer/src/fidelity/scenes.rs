//! The fidelity scenes. Each is small, lit one way, and seen from one place;
//! the names are what the references and goldens are filed under.

use rbx_dom::Variant;

use super::{
    at, content, float, rgb, tilted_x, Builder, Scene, BALL, BLOCK, CYLINDER, PLASTIC,
    WEDGE,
};

const SMOOTH_PLASTIC: u32 = 272;
const NEON: u32 = 288;
const GLASS: u32 = 1568;
const FORCE_FIELD: u32 = 1584;
const METAL: u32 = 1088;
const FOIL: u32 = 1072;

/// Every `Enum.Material` a part can be, but the ones [`special`] covers.
const MATERIALS: [(&str, u32); 39] = [
    ("Plastic", 256),
    ("SmoothPlastic", 272),
    ("Wood", 512),
    ("WoodPlanks", 528),
    ("Marble", 784),
    ("Basalt", 788),
    ("Slate", 800),
    ("CrackedLava", 804),
    ("Concrete", 816),
    ("Limestone", 820),
    ("Granite", 832),
    ("Pavement", 836),
    ("Brick", 848),
    ("Pebble", 864),
    ("Cobblestone", 880),
    ("Rock", 896),
    ("Sandstone", 912),
    ("CorrodedMetal", 1040),
    ("DiamondPlate", 1056),
    ("Foil", 1072),
    ("Metal", 1088),
    ("Grass", 1280),
    ("LeafyGrass", 1284),
    ("Sand", 1296),
    ("Fabric", 1312),
    ("Snow", 1328),
    ("Mud", 1344),
    ("Ground", 1360),
    ("Asphalt", 1376),
    ("Salt", 1392),
    ("Ice", 1536),
    ("Glacier", 1552),
    ("Cardboard", 2304),
    ("Carpet", 2305),
    ("CeramicTiles", 2306),
    ("ClayRoofTiles", 2307),
    ("RoofShingles", 2308),
    ("Leather", 2309),
    ("Plaster", 2310),
];

/// Looking at [`stage`] from the front and a little above, the horizon a
/// third of the way down.
const STAGE_EYE: [f32; 3] = [0.0, 7.0, 24.0];
const STAGE_AT: [f32; 3] = [0.0, 3.0, 0.0];

/// [`stage`]'s faces as [`STAGE_EYE`] sees them, a lit and a shadowed patch
/// of floor and two points of sky.
const STAGE_PROBES: [(&str, [f32; 3]); 16] = [
    ("sky", [-600.0, 300.0, -2000.0]),
    ("high sky", [0.0, 700.0, -2000.0]),
    ("floor", [-12.0, 0.0, 14.0]),
    ("floor shadow", [11.5, 0.0, 3.0]),
    ("white top", [-9.0, 4.0, 0.0]),
    ("white front", [-10.3, 2.5, 2.0]),
    ("grey top", [-3.0, 4.0, 0.0]),
    ("grey front", [-3.0, 2.0, 2.0]),
    ("red top", [3.0, 4.0, 0.0]),
    ("red front", [3.5, 2.5, 2.0]),
    ("green top", [9.0, 4.0, 0.0]),
    ("green front", [9.0, 2.8, 2.0]),
    ("blue ball", [-5.36, 2.52, 8.82]),
    ("yellow cylinder", [0.0, 2.5, 8.94]),
    ("black wedge", [6.0, 2.0, 7.0]),
    ("pole", [0.0, 8.0, -7.5]),
];

/// The shadow scenes: lit floor, the shadows of a pole and of the slab, the
/// tilted board and the ball.
const SHADOW_PROBES: [(&str, [f32; 3]); 5] = [
    ("lit floor", [-12.0, 0.0, 12.0]),
    ("pole shadow", [-6.2, 0.0, -3.25]),
    ("slab shadow", [11.0, 0.0, -0.4]),
    ("red board", [-4.0, 3.0, 6.0]),
    ("white ball", [4.0, 2.2, 7.3]),
];

/// [`special`]'s pieces from [`STAGE_EYE`].
const SPECIAL_PROBES: [(&str, [f32; 3]); 9] = [
    ("red wall", [0.0, 7.0, -5.5]),
    ("glass", [-8.0, 2.0, 2.0]),
    ("neon block", [-3.0, 2.0, 2.0]),
    ("neon cylinder", [-3.0, 2.5, 6.94]),
    ("force field", [2.0, 2.0, 2.0]),
    ("reflective ball", [6.5, 2.4, 1.9]),
    ("see-through pair", [3.0, 2.0, 8.0]),
    ("metal ball", [9.32, 1.87, 6.3]),
    ("foil", [-9.0, 1.5, 7.5]),
];

/// What Studio's Baseplate template writes into `Lighting` (without its
/// `Sky`, `Atmosphere` and effects): the look most places start from.
fn studio() -> Vec<(&'static str, Variant)> {
    vec![
        ("Ambient", rgb(70, 70, 70)),
        ("OutdoorAmbient", rgb(70, 70, 70)),
        ("Brightness", float(3.0)),
        ("ClockTime", float(14.5)),
        ("GeographicLatitude", float(0.0)),
        ("EnvironmentDiffuseScale", float(1.0)),
        ("EnvironmentSpecularScale", float(1.0)),
        ("GlobalShadows", Variant::Bool(true)),
        ("ShadowSoftness", float(0.2)),
        ("Technology", Variant::Enum(3)),
        ("LightingStyle", Variant::Enum(1)),
        ("PrioritizeLightingQuality", Variant::Bool(true)),
    ]
}

fn with(
    mut base: Vec<(&'static str, Variant)>,
    changes: &[(&'static str, Variant)],
) -> Vec<(&'static str, Variant)> {
    for (key, value) in changes {
        base.retain(|(k, _)| k != key);
        base.push((key, value.clone()));
    }
    base
}

/// The floor and a row of plain blocks in the colours places use most,
/// with a ball, a cylinder, a wedge and a pole behind for its shadow.
fn stage(b: &mut Builder) {
    b.floor();
    b.block([4.0; 3], [-9.0, 2.0, 0.0], [248, 248, 248]);
    b.block([4.0; 3], [-3.0, 2.0, 0.0], [163, 162, 165]);
    b.block([4.0; 3], [3.0, 2.0, 0.0], [196, 40, 28]);
    b.block([4.0; 3], [9.0, 2.0, 0.0], [75, 151, 75]);
    b.part(BALL, [4.0; 3], at([-6.0, 2.0, 7.0]), [13, 105, 172], PLASTIC);
    b.part(CYLINDER, [4.0; 3], at([0.0, 2.0, 7.0]), [245, 205, 48], PLASTIC);
    b.part(WEDGE, [4.0; 3], at([6.0, 2.0, 7.0]), [27, 42, 53], PLASTIC);
    b.block([1.0, 12.0, 1.0], [0.0, 6.0, -8.0], [248, 248, 248]);
}

/// The Baseplate template's `Sky`, `Atmosphere`, `SunRays` and `Bloom`.
fn template_sky(b: &mut Builder) {
    let sky = b.lighting_child("Sky");
    for key in ["SkyboxBk", "SkyboxFt", "SkyboxLf", "SkyboxRt"] {
        b.set(sky, key, content("rbxassetid://6444884337"));
    }
    b.set(sky, "SkyboxDn", content("rbxassetid://6444884785"));
    b.set(sky, "SkyboxUp", content("rbxassetid://6412503613"));
    b.set(sky, "SunTextureId", content("rbxassetid://6196665106"));
    b.set(sky, "MoonTextureId", content("rbxassetid://6444320592"));
    b.set(sky, "SunAngularSize", float(11.0));
    b.set(sky, "MoonAngularSize", float(11.0));
    b.set(sky, "StarCount", Variant::Int32(3000));
}

fn template_effects(b: &mut Builder) {
    template_sky(b);
    let rays = b.lighting_child("SunRaysEffect");
    b.set(rays, "Intensity", float(0.01));
    b.set(rays, "Spread", float(0.1));
    let atmosphere = b.lighting_child("Atmosphere");
    b.set(atmosphere, "Color", rgb(199, 199, 199));
    b.set(atmosphere, "Decay", rgb(106, 112, 125));
    b.set(atmosphere, "Density", float(0.3));
    b.set(atmosphere, "Offset", float(0.25));
    b.set(atmosphere, "Glare", float(0.0));
    b.set(atmosphere, "Haze", float(0.0));
    let bloom = b.lighting_child("BloomEffect");
    b.set(bloom, "Size", float(24.0));
    b.set(bloom, "Intensity", float(1.0));
    b.set(bloom, "Threshold", float(2.0));
}

/// The top centre of the `i`th slab of a [`materials`] grid.
fn slab(i: usize) -> [f32; 3] {
    let (column, row) = ((i % 5) as f32, (i / 5) as f32);
    [(column - 2.0) * 6.0, 1.0, (row - 1.5) * 6.0]
}

fn materials(b: &mut Builder, from: usize) {
    b.floor();
    for (i, &(_, material)) in MATERIALS.iter().skip(from).take(20).enumerate() {
        let [x, _, z] = slab(i);
        b.part(BLOCK, [5.0, 1.0, 5.0], at([x, 0.5, z]), [163, 162, 165], material);
    }
}

/// Glass, neon, a force field, reflectance and two overlapping see-through
/// blocks, in front of things to see through them to.
fn special(b: &mut Builder) {
    b.floor();
    b.block([20.0, 8.0, 1.0], [0.0, 4.0, -6.0], [196, 40, 28]);
    b.part(BLOCK, [4.0; 3], at([-8.0, 2.0, 0.0]), [163, 162, 165], GLASS);
    b.part(BLOCK, [4.0; 3], at([-3.0, 2.0, 0.0]), [255, 0, 191], NEON);
    b.part(CYLINDER, [2.0, 4.0, 4.0], at([-3.0, 2.0, 5.0]), [4, 175, 236], NEON);
    b.part(BLOCK, [4.0; 3], at([2.0, 2.0, 0.0]), [4, 175, 236], FORCE_FIELD);
    let mirror = b.part(BALL, [4.0; 3], at([7.0, 2.0, 0.0]), [163, 162, 165], SMOOTH_PLASTIC);
    b.set(mirror, "Reflectance", float(0.6));
    let front = b.block([4.0; 3], [3.0, 2.0, 6.0], [248, 248, 248]);
    b.set(front, "Transparency", float(0.5));
    let back = b.block([4.0; 3], [5.0, 2.5, 4.0], [13, 105, 172]);
    b.set(back, "Transparency", float(0.5));
    b.part(BALL, [3.0; 3], at([10.0, 1.5, 5.0]), [163, 162, 165], METAL);
    b.part(BLOCK, [3.0; 3], at([-9.0, 1.5, 6.0]), [163, 162, 165], FOIL);
}

/// Pillars receding into the distance, for fog and atmosphere.
fn avenue(b: &mut Builder) {
    b.floor();
    for i in 0..12 {
        let z = -20.0 * i as f32;
        b.block([3.0, 16.0, 3.0], [-10.0, 8.0, z], [248, 248, 248]);
        b.block([3.0, 16.0, 3.0], [10.0, 8.0, z], [196, 40, 28]);
    }
}

/// A light inside an invisible holder part.
fn lamp(b: &mut Builder, class: &str, position: [f32; 3], props: &[(&str, Variant)]) {
    let holder = b.block([1.0; 3], position, [248, 248, 248]);
    b.set(holder, "Transparency", float(1.0));
    b.set(holder, "CastShadow", Variant::Bool(false));
    let light = b.child(holder, class);
    for (key, value) in props {
        b.set(light, key, value.clone());
    }
}

fn local_lights(b: &mut Builder) {
    b.floor();
    b.block([2.0, 4.0, 2.0], [-6.0, 2.0, 1.0], [248, 248, 248]);
    lamp(
        b,
        "PointLight",
        [-8.0, 3.0, 3.0],
        &[
            ("Range", float(16.0)),
            ("Brightness", float(2.0)),
            ("Color", rgb(255, 170, 80)),
            ("Shadows", Variant::Bool(true)),
        ],
    );
    lamp(
        b,
        "SpotLight",
        [2.0, 9.0, 0.0],
        &[
            ("Range", float(16.0)),
            ("Brightness", float(4.0)),
            ("Angle", float(45.0)),
            ("Face", Variant::Enum(4)),
            ("Shadows", Variant::Bool(true)),
        ],
    );
    b.block([1.0, 3.0, 1.0], [2.0, 1.5, 0.0], [196, 40, 28]);
    let wall = b.block([6.0, 8.0, 1.0], [9.0, 4.0, -4.0], [163, 162, 165]);
    let light = b.child(wall, "SurfaceLight");
    b.set(light, "Face", Variant::Enum(2));
    b.set(light, "Range", float(12.0));
    b.set(light, "Brightness", float(3.0));
    b.set(light, "Angle", float(60.0));
    b.set(light, "Color", rgb(120, 180, 255));
}

fn effect(b: &mut Builder, class: &str, props: &[(&str, Variant)]) {
    let effect = b.lighting_child(class);
    for (key, value) in props {
        b.set(effect, key, value.clone());
    }
}

/// Toward the sun at 17:00 on the equator: low in the west and a little
/// south.
const SUNSET_EYE: [f32; 3] = [10.0, 6.0, 10.0];
const SUNSET_AT: [f32; 3] = [-78.0, 18.0, -30.0];

pub fn scenes() -> Vec<Scene> {
    let stage_scene = |name, about, lighting, build: fn(&mut Builder)| Scene {
        name,
        about,
        eye: STAGE_EYE,
        look_at: STAGE_AT,
        fov: 70.0,
        lighting,
        probes: STAGE_PROBES.to_vec(),
        build,
    };
    let sunset = |name, about, lighting, build| Scene {
        name,
        about,
        eye: SUNSET_EYE,
        look_at: SUNSET_AT,
        fov: 70.0,
        lighting,
        probes: vec![
            ("sun", [-2648.0, 717.0, -1187.0]),
            ("sky", [-2400.0, 1400.0, 600.0]),
            ("horizon", [-2900.0, 100.0, 300.0]),
            ("floor", [-20.0, 0.0, -5.0]),
            ("white pillar", [-8.5, 8.0, 0.0]),
            ("far pillar", [-8.5, 8.0, -120.0]),
        ],
        build,
    };
    let material_scene = |name, about, from: usize, build| Scene {
        name,
        about,
        eye: [0.0, 26.0, 22.0],
        look_at: [0.0, 0.0, 1.0],
        fov: 70.0,
        lighting: studio(),
        probes: MATERIALS
            .iter()
            .skip(from)
            .take(20)
            .enumerate()
            .map(|(i, &(name, _))| (name, slab(i)))
            .collect(),
        build,
    };
    vec![
        stage_scene(
            "default_lighting",
            "Lighting left at its class defaults, no Sky: Roblox's own defaults",
            vec![],
            stage,
        ),
        stage_scene(
            "studio_baseplate",
            "Studio's Baseplate template lighting with its Sky, Atmosphere, SunRays and Bloom",
            studio(),
            |b| {
                stage(b);
                template_effects(b);
            },
        ),
        stage_scene("studio_lighting", "The template's Lighting values alone, default sky", studio(), stage),
        stage_scene(
            "morning",
            "ClockTime 7 on the equator: a low sun in the east",
            with(studio(), &[("ClockTime", float(7.0))]),
            stage,
        ),
        stage_scene(
            "evening",
            "ClockTime 17.8: sunset colours",
            with(studio(), &[("ClockTime", float(17.8))]),
            stage,
        ),
        stage_scene(
            "night",
            "ClockTime 0: moon, stars and the night ambient",
            with(studio(), &[("ClockTime", float(0.0))]),
            stage,
        ),
        stage_scene(
            "latitude",
            "GeographicLatitude 60 at noon",
            with(
                studio(),
                &[("ClockTime", float(12.0)), ("GeographicLatitude", float(60.0))],
            ),
            stage,
        ),
        stage_scene(
            "ambient_colour",
            "Coloured Ambient, OutdoorAmbient and ColorShift, Brightness 1, no environment light",
            with(
                studio(),
                &[
                    ("Ambient", rgb(120, 60, 60)),
                    ("OutdoorAmbient", rgb(40, 60, 120)),
                    ("Brightness", float(1.0)),
                    ("ColorShift_Top", rgb(255, 200, 150)),
                    ("ColorShift_Bottom", rgb(0, 50, 100)),
                    ("EnvironmentDiffuseScale", float(0.0)),
                    ("EnvironmentSpecularScale", float(0.0)),
                ],
            ),
            stage,
        ),
        stage_scene(
            "exposure",
            "ExposureCompensation 1.5",
            with(studio(), &[("ExposureCompensation", float(1.5))]),
            stage,
        ),
        stage_scene(
            "realistic_style",
            "LightingStyle Realistic",
            with(studio(), &[("LightingStyle", Variant::Enum(0))]),
            stage,
        ),
        Scene {
            name: "fog",
            about: "Linear fog: FogStart 10, FogEnd 140, FogColor warm",
            eye: [0.0, 6.0, 20.0],
            look_at: [0.0, 5.0, -100.0],
            fov: 70.0,
            lighting: with(
                studio(),
                &[
                    ("FogStart", float(10.0)),
                    ("FogEnd", float(140.0)),
                    ("FogColor", rgb(190, 140, 90)),
                ],
            ),
            probes: vec![
                ("sky", [0.0, 300.0, -3000.0]),
                ("floor", [0.0, 0.0, 0.0]),
                ("near white", [-8.5, 8.0, 0.0]),
                ("mid white", [-8.5, 8.0, -40.0]),
                ("far white", [-8.5, 8.0, -100.0]),
                ("far red", [8.5, 8.0, -100.0]),
            ],
            build: avenue,
        },
        sunset(
            "atmosphere",
            "Atmosphere Density 0.45, Offset 0.3, Glare 0.6, Haze 1.5 toward a 17:00 sun",
            with(
                studio(),
                &[("ClockTime", float(17.0)), ("GeographicLatitude", float(0.0))],
            ),
            |b| {
                avenue(b);
                let a = b.lighting_child("Atmosphere");
                b.set(a, "Density", float(0.45));
                b.set(a, "Offset", float(0.3));
                b.set(a, "Color", rgb(199, 170, 107));
                b.set(a, "Decay", rgb(92, 60, 13));
                b.set(a, "Glare", float(0.6));
                b.set(a, "Haze", float(1.5));
            },
        ),
        sunset(
            "sun_rays",
            "SunRaysEffect Intensity 0.3, Spread 0.8 toward a 17:00 sun past pillars",
            with(studio(), &[("ClockTime", float(17.0))]),
            |b| {
                avenue(b);
                effect(
                    b,
                    "SunRaysEffect",
                    &[("Intensity", float(0.3)), ("Spread", float(0.8))],
                );
            },
        ),
        Scene {
            name: "custom_sky",
            about: "The template's Sky skybox, looking up",
            eye: [0.0, 5.0, 0.0],
            look_at: [0.0, 25.0, -40.0],
            fov: 70.0,
            lighting: studio(),
            probes: vec![
                ("up", [0.0, 860.0, -600.0]),
                ("horizon", [0.0, 155.0, -1000.0]),
                ("left", [-700.0, 400.0, -700.0]),
                ("right", [700.0, 400.0, -700.0]),
            ],
            build: |b| {
                b.floor();
                template_sky(b);
            },
        },
        Scene {
            name: "shadows_hard",
            about: "ShadowSoftness 0: poles and a floating slab",
            eye: [0.0, 14.0, 22.0],
            look_at: [0.0, 0.0, -2.0],
            fov: 70.0,
            lighting: with(studio(), &[("ShadowSoftness", float(0.0))]),
            probes: SHADOW_PROBES.to_vec(),
            build: shadows,
        },
        Scene {
            name: "shadows_soft",
            about: "ShadowSoftness 1: poles and a floating slab",
            eye: [0.0, 14.0, 22.0],
            look_at: [0.0, 0.0, -2.0],
            fov: 70.0,
            lighting: with(studio(), &[("ShadowSoftness", float(1.0))]),
            probes: SHADOW_PROBES.to_vec(),
            build: shadows,
        },
        Scene {
            name: "local_lights",
            about: "Point, spot and surface lights with shadows at night",
            eye: [0.0, 10.0, 20.0],
            look_at: [0.0, 1.0, 0.0],
            fov: 70.0,
            lighting: with(
                studio(),
                &[
                    ("ClockTime", float(0.0)),
                    ("Ambient", rgb(0, 0, 0)),
                    ("OutdoorAmbient", rgb(20, 20, 30)),
                ],
            ),
            probes: vec![
                ("point floor", [-8.0, 0.0, 5.0]),
                ("lit block", [-6.0, 2.5, 2.0]),
                ("spot floor", [4.0, 0.0, 2.0]),
                ("surface floor", [9.0, 0.0, 0.0]),
                ("dark floor", [-14.0, 0.0, 12.0]),
            ],
            build: local_lights,
        },
        material_scene("materials_a", "The first 20 materials on grey slabs", 0, |b| {
            materials(b, 0)
        }),
        material_scene("materials_b", "The other 19 materials on grey slabs", 20, |b| {
            materials(b, 20)
        }),
        Scene {
            probes: SPECIAL_PROBES.to_vec(),
            ..stage_scene(
                "special_materials",
                "Glass, Neon, ForceField, Reflectance, Metal, Foil and overlapping transparency",
                studio(),
                special,
            )
        },
        Scene {
            probes: SPECIAL_PROBES.to_vec(),
            ..stage_scene(
                "bloom_colour_correction",
                "Bloom (threshold 0.8) on neon, ColorCorrection tint, contrast and saturation",
                studio(),
                |b| {
                    special(b);
                    effect(
                        b,
                        "BloomEffect",
                        &[
                            ("Intensity", float(1.0)),
                            ("Size", float(24.0)),
                            ("Threshold", float(0.8)),
                        ],
                    );
                    effect(
                        b,
                        "ColorCorrectionEffect",
                        &[
                            ("Brightness", float(0.05)),
                            ("Contrast", float(0.2)),
                            ("Saturation", float(-0.3)),
                            ("TintColor", rgb(255, 235, 210)),
                        ],
                    );
                },
            )
        },
        stage_scene("blur", "BlurEffect Size 10", studio(), |b| {
            stage(b);
            effect(b, "BlurEffect", &[("Size", float(10.0))]);
        }),
        stage_scene(
            "depth_of_field",
            "DepthOfField focused at 15 studs, radius 6, far 0.8, near 0.6",
            studio(),
            |b| {
                stage(b);
                b.block([2.0; 3], [-2.0, 4.0, 19.0], [245, 205, 48]);
                effect(
                    b,
                    "DepthOfFieldEffect",
                    &[
                        ("FarIntensity", float(0.8)),
                        ("FocusDistance", float(15.0)),
                        ("InFocusRadius", float(6.0)),
                        ("NearIntensity", float(0.6)),
                    ],
                );
            },
        ),
    ]
    .into_iter()
    .chain(tone_scenes())
    .chain(falloff_scenes())
    .collect()
}

/// The grey levels of [`ramp`], 0 to 255 in steps of 17.
const RAMP: usize = 16;
const RAMP_TOPS: [&str; RAMP] = [
    "0 top", "17 top", "34 top", "51 top", "68 top", "85 top", "102 top", "119 top", "136 top",
    "153 top", "170 top", "187 top", "204 top", "221 top", "238 top", "255 top",
];
const RAMP_FRONTS: [&str; RAMP] = [
    "0 front", "17 front", "34 front", "51 front", "68 front", "85 front", "102 front",
    "119 front", "136 front", "153 front", "170 front", "187 front", "204 front", "221 front",
    "238 front", "255 front",
];

fn ramp_x(i: usize) -> f32 {
    (i as f32 - 7.5) * 2.5
}

/// Sixteen grey cubes from black to white, floating, no floor.
fn ramp(b: &mut Builder) {
    for i in 0..RAMP {
        let grey = (i * 17) as u8;
        b.block([2.0; 3], [ramp_x(i), 0.0, 0.0], [grey; 3]);
    }
}

/// The sun straight overhead (noon at latitude 23.5), nothing else lighting
/// the ramp unless a scene adds it: what turns a top face's colour into the
/// renderer's response to `grey × light`.
fn tone(brightness: f32, changes: &[(&'static str, Variant)]) -> Vec<(&'static str, Variant)> {
    let base = vec![
        ("Ambient", rgb(0, 0, 0)),
        ("OutdoorAmbient", rgb(0, 0, 0)),
        ("Brightness", float(brightness)),
        ("ClockTime", float(12.0)),
        ("GeographicLatitude", float(23.5)),
        ("EnvironmentDiffuseScale", float(0.0)),
        ("EnvironmentSpecularScale", float(0.0)),
        ("GlobalShadows", Variant::Bool(false)),
    ];
    with(base, changes)
}

/// The tone-response scenes: the grey ramp under a known light, from the
/// front and above, so each top face is one sample of the response.
fn tone_scenes() -> Vec<Scene> {
    let place_level = |mut lighting: Vec<(&'static str, Variant)>, studio_place: bool| {
        if studio_place {
            lighting.extend(
                studio()
                    .into_iter()
                    .filter(|(k, _)| super::roblox::PLACE_LEVEL.contains(k)),
            );
        }
        lighting
    };
    let scene = |name, about, lighting, studio_place| Scene {
        name,
        about,
        eye: [0.0, 9.0, 16.0],
        look_at: [0.0, 0.0, 0.0],
        fov: 70.0,
        lighting: place_level(lighting, studio_place),
        probes: (0..RAMP)
            .flat_map(|i| {
                [
                    (RAMP_TOPS[i], [ramp_x(i), 1.0, 0.0]),
                    (RAMP_FRONTS[i], [ramp_x(i), 0.0, 1.0]),
                ]
            })
            .chain([("sky", [-500.0, -150.0, -1000.0])])
            .collect(),
        build: ramp,
    };
    vec![
        scene("tone_sun_1", "The ramp under an overhead sun, Brightness 1", tone(1.0, &[]), true),
        scene("tone_sun_2", "The ramp under an overhead sun, Brightness 2", tone(2.0, &[]), true),
        scene("tone_sun_4", "The ramp under an overhead sun, Brightness 4", tone(4.0, &[]), true),
        scene("tone_sun_8", "The ramp under an overhead sun, Brightness 8", tone(8.0, &[]), true),
        scene(
            "tone_ambient",
            "The ramp under white OutdoorAmbient alone",
            tone(0.0, &[("OutdoorAmbient", rgb(255, 255, 255))]),
            true,
        ),
        scene(
            "tone_ambient_half",
            "The ramp under 128 grey Ambient alone",
            tone(0.0, &[("Ambient", rgb(128, 128, 128))]),
            true,
        ),
        scene(
            "tone_sky",
            "The ramp under the default sky's light alone (EnvironmentDiffuseScale 1)",
            tone(0.0, &[("EnvironmentDiffuseScale", float(1.0))]),
            true,
        ),
        scene(
            "tone_exposure",
            "The ramp under an overhead sun, Brightness 1, ExposureCompensation 2",
            tone(1.0, &[("ExposureCompensation", float(2.0))]),
            true,
        ),
        // The sun lower in the sky, Brightness 2: at latitude 23.5 the sun is
        // `15 × (ClockTime − 6)` degrees up, so a top face's colour against
        // tone_sun_2's says what the sun loses on the way down.
        scene(
            "tone_sun_60",
            "tone_sun_2 with the sun 60 degrees up",
            tone(2.0, &[("ClockTime", float(10.0))]),
            true,
        ),
        scene(
            "tone_sun_30",
            "tone_sun_2 with the sun 30 degrees up",
            tone(2.0, &[("ClockTime", float(8.0))]),
            true,
        ),
        scene(
            "tone_sun_15",
            "tone_sun_2 with the sun 15 degrees up",
            tone(2.0, &[("ClockTime", float(7.0))]),
            true,
        ),
        scene(
            "tone_sun_5",
            "tone_sun_2 with the sun 5 degrees up",
            tone(2.0, &[("ClockTime", float(6.0 + 1.0 / 3.0))]),
            true,
        ),
        scene(
            "tone_sun_2_voxel",
            "tone_sun_2 in a place that leaves Technology and LightingStyle at their defaults",
            tone(2.0, &[]),
            false,
        ),
        scene(
            "tone_ambient_voxel",
            "tone_ambient in a place that leaves Technology and LightingStyle at their defaults",
            tone(0.0, &[("OutdoorAmbient", rgb(255, 255, 255))]),
            false,
        ),
    ]
}

/// Thin poles, a slab on a stilt and a tilted board for the sun's shadows.
fn shadows(b: &mut Builder) {
    b.floor();
    for i in 0..5 {
        b.block([0.5, 10.0, 0.5], [-10.0 + 3.0 * i as f32, 5.0, -6.0], [248, 248, 248]);
    }
    b.block([1.0, 6.0, 1.0], [6.0, 3.0, -4.0], [163, 162, 165]);
    b.block([8.0, 0.5, 8.0], [6.0, 6.25, -4.0], [163, 162, 165]);
    b.part(BLOCK, [6.0, 0.5, 6.0], tilted_x([-4.0, 3.0, 6.0], 30.0), [196, 40, 28], PLASTIC);
    b.part(BALL, [3.0; 3], at([4.0, 1.5, 6.0]), [248, 248, 248], PLASTIC);
}

/// Floor points every two studs out from below a local light, for
/// [`falloff_scenes`].
const FALLOFF: [&str; 11] = ["0", "2", "4", "6", "8", "10", "12", "14", "16", "18", "20"];

/// A white floor at night with nothing else lit, so the floor's colour at
/// each distance is the light's falloff alone.
fn dark_floor(b: &mut Builder) {
    b.block([80.0, 1.0, 80.0], [0.0, -0.5, 0.0], [248, 248, 248]);
}

/// One local light over [`dark_floor`], its floor sampled every two studs
/// along +X from below it (x = -10).
fn falloff_scenes() -> Vec<Scene> {
    let lighting = || {
        tone(
            0.0,
            &[
                ("ClockTime", float(0.0)),
                ("EnvironmentDiffuseScale", float(0.0)),
            ],
        )
        .into_iter()
        .chain(
            studio()
                .into_iter()
                .filter(|(k, _)| super::roblox::PLACE_LEVEL.contains(k)),
        )
        .collect()
    };
    let scene = |name, about, build| Scene {
        name,
        about,
        eye: [0.0, 30.0, 25.0],
        look_at: [0.0, 0.0, 0.0],
        fov: 70.0,
        lighting: lighting(),
        probes: FALLOFF
            .iter()
            .enumerate()
            .map(|(i, &name)| (name, [-10.0 + 2.0 * i as f32, 0.0, 0.0]))
            .collect(),
        build,
    };
    vec![
        scene(
            "light_point_1",
            "PointLight Range 16 Brightness 1 two studs over a white floor",
            |b| point_over_floor(b, 1.0),
        ),
        scene(
            "light_point_4",
            "PointLight Range 16 Brightness 4 two studs over a white floor",
            |b| point_over_floor(b, 4.0),
        ),
        scene(
            "light_spot",
            "SpotLight Range 16 Brightness 2 Angle 90 six studs up, facing down",
            |b| {
                dark_floor(b);
                lamp(
                    b,
                    "SpotLight",
                    [-10.0, 6.0, 0.0],
                    &[
                        ("Range", float(16.0)),
                        ("Brightness", float(2.0)),
                        ("Angle", float(90.0)),
                        ("Face", Variant::Enum(4)),
                    ],
                );
            },
        ),
        scene(
            "light_surface",
            "SurfaceLight Range 16 Brightness 2 Angle 90 on a wall's Right face",
            |b| {
                dark_floor(b);
                let wall = b.block([1.0, 6.0, 6.0], [-10.5, 3.0, 0.0], [163, 162, 165]);
                let light = b.child(wall, "SurfaceLight");
                b.set(light, "Face", Variant::Enum(0));
                b.set(light, "Range", float(16.0));
                b.set(light, "Brightness", float(2.0));
                b.set(light, "Angle", float(90.0));
            },
        ),
    ]
}

fn point_over_floor(b: &mut Builder, brightness: f32) {
    dark_floor(b);
    lamp(
        b,
        "PointLight",
        [-10.0, 2.0, 0.0],
        &[("Range", float(16.0)), ("Brightness", float(brightness))],
    );
}
