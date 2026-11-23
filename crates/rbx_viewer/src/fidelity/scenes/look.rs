//! Scenes of what a player sees first in a new place: the Baseplate
//! template's textured floor and spawn, the noob, and `Decal`/`Texture`
//! children drawn on parts.

use rbx_dom::{Variant, Vector3Data};

use super::super::{
    at, content, float, rgb, Builder, Generated, GeneratedImage, Scene, BLOCK, PLASTIC,
};
use super::{studio, template_effects};

/// The Baseplate template's stud grid tile, a public image.
const GRID: &str = "rbxassetid://6372755229";
const SPAWN_DECAL: &str = "rbxasset://textures/SpawnLocation.png";
const FACE: &str = "rbxasset://textures/face.png";
/// `NormalId`s.
const TOP: u32 = 1;
const FRONT: u32 = 5;

/// The Baseplate template's floor and `SpawnLocation`, with the template's
/// `Texture` and `Decal`.
fn template_world(b: &mut Builder) {
    let floor = b.block([2048.0, 16.0, 2048.0], [0.0, -8.0, 0.0], [91, 91, 91]);
    let texture = b.child(floor, "Texture");
    b.set(texture, "Texture", content(GRID));
    b.set(texture, "Face", Variant::Enum(TOP));
    b.set(texture, "Transparency", float(0.8));
    b.set(texture, "StudsPerTileU", float(8.0));
    b.set(texture, "StudsPerTileV", float(8.0));
    b.set(texture, "Color3", rgb(0, 0, 0));
    let spawn = b.block([12.0, 1.0, 12.0], [0.0, 0.5, 0.0], [163, 162, 165]);
    let decal = b.child(spawn, "Decal");
    b.set(decal, "Texture", content(SPAWN_DECAL));
    b.set(decal, "Face", Variant::Enum(TOP));
}

/// The classic noob as an R6 figure standing at `x`, facing -Z: Bright yellow head and
/// arms, Bright blue torso, Br. yellowish green legs, the round head mesh
/// and the default face.
fn noob(b: &mut Builder, x: f32) {
    const YELLOW: [u8; 3] = [245, 205, 48];
    const BLUE: [u8; 3] = [13, 105, 172];
    const GREEN: [u8; 3] = [164, 189, 71];
    let limbs: [([f32; 3], [f32; 3], [u8; 3]); 5] = [
        ([2.0, 2.0, 1.0], [0.0, 3.0, 0.0], BLUE),
        ([1.0, 2.0, 1.0], [-1.5, 3.0, 0.0], YELLOW),
        ([1.0, 2.0, 1.0], [1.5, 3.0, 0.0], YELLOW),
        ([1.0, 2.0, 1.0], [-0.5, 1.0, 0.0], GREEN),
        ([1.0, 2.0, 1.0], [0.5, 1.0, 0.0], GREEN),
    ];
    for (size, [px, py, pz], colour) in limbs {
        b.block(size, [px + x, py, pz], colour);
    }
    let head = b.block([2.0, 1.0, 1.0], [x, 4.5, 0.0], YELLOW);
    let mesh = b.child(head, "SpecialMesh");
    b.set(mesh, "MeshType", Variant::Enum(0));
    b.set(
        mesh,
        "Scale",
        Variant::Vector3(Vector3Data {
            x: 1.25,
            y: 1.25,
            z: 1.25,
        }),
    );
    let face = b.child(head, "Decal");
    b.set(face, "Texture", content(FACE));
    b.set(face, "Face", Variant::Enum(FRONT));
}

/// Seen from the front (-Z), a wall of three blocks: a stretched `Decal`, a `Texture` tiled every 2
/// studs and offset, and a tinted, half transparent `Texture`.
fn decals(b: &mut Builder) {
    b.block([2048.0, 16.0, 2048.0], [0.0, -8.0, 0.0], [91, 91, 91]);
    let plain = b.block([6.0, 6.0, 1.0], [-7.0, 3.0, 0.0], [248, 248, 248]);
    let decal = b.child(plain, "Decal");
    b.set(decal, "Texture", content(GRID));
    b.set(decal, "Face", Variant::Enum(FRONT));
    let tiled = b.block([6.0, 6.0, 1.0], [0.0, 3.0, 0.0], [248, 248, 248]);
    let texture = b.child(tiled, "Texture");
    b.set(texture, "Texture", content(GRID));
    b.set(texture, "Face", Variant::Enum(FRONT));
    b.set(texture, "StudsPerTileU", float(2.0));
    b.set(texture, "StudsPerTileV", float(2.0));
    b.set(texture, "OffsetStudsU", float(0.5));
    b.set(texture, "OffsetStudsV", float(1.0));
    let tinted = b.part(
        BLOCK,
        [6.0, 6.0, 1.0],
        at([7.0, 3.0, 0.0]),
        [196, 40, 28],
        PLASTIC,
    );
    let texture = b.child(tinted, "Texture");
    b.set(texture, "Texture", content(GRID));
    b.set(texture, "Face", Variant::Enum(FRONT));
    b.set(texture, "StudsPerTileU", float(3.0));
    b.set(texture, "StudsPerTileV", float(3.0));
    b.set(texture, "Transparency", float(0.5));
    b.set(texture, "Color3", rgb(0, 0, 255));
}

/// `Texture` transparencies 0 to 0.75 over red parts, blue tinted along the
/// top row and untinted below, each tiled at 12 studs so a 6 stud face
/// shows one flat quarter of the grid (white at alpha 147).
fn texture_blend(b: &mut Builder) {
    b.block([2048.0, 16.0, 2048.0], [0.0, -8.0, 0.0], [91, 91, 91]);
    for (row, tint) in [[0, 0, 255], [255, 255, 255]].into_iter().enumerate() {
        for (column, transparency) in [0.0, 0.25, 0.5, 0.75].into_iter().enumerate() {
            let x = -10.5 + 7.0 * column as f32;
            let y = 3.0 + 7.0 * (1 - row) as f32;
            let part = b.part(
                BLOCK,
                [6.0, 6.0, 1.0],
                at([x, y, 0.0]),
                [196, 40, 28],
                PLASTIC,
            );
            let texture = b.child(part, "Texture");
            b.set(texture, "Texture", content(GRID));
            b.set(texture, "Face", Variant::Enum(FRONT));
            b.set(texture, "StudsPerTileU", float(12.0));
            b.set(texture, "StudsPerTileV", float(12.0));
            b.set(texture, "Transparency", float(transparency));
            let [r, g, bl] = tint;
            b.set(texture, "Color3", rgb(r, g, bl));
        }
    }
}

/// `Enum.Material.Metal`.
const METAL: u32 = 1088;

/// `MaterialVariant`s with the grid as their colour map, on blocks: plain at
/// 4 studs a tile, `Pattern = Organic` at 8, tinted by the part's colour, and
/// on a `Metal` base with the grid as its metalness map too.
fn material_variants(b: &mut Builder) {
    b.block([2048.0, 16.0, 2048.0], [0.0, -8.0, 0.0], [91, 91, 91]);
    let variants: [(&str, u32, f32, u32, [u8; 3]); 4] = [
        ("Grid", PLASTIC, 4.0, 0, [248, 248, 248]),
        ("Organic", PLASTIC, 8.0, 1, [248, 248, 248]),
        ("Tinted", PLASTIC, 4.0, 0, [196, 40, 28]),
        ("Metallic", METAL, 4.0, 0, [163, 162, 165]),
    ];
    for (i, (name, base, studs, pattern, colour)) in variants.into_iter().enumerate() {
        let variant = b.material_variant(name);
        b.set(variant, "BaseMaterial", Variant::Enum(base));
        b.set(variant, "ColorMap", content(GRID));
        b.set(variant, "StudsPerTile", float(studs));
        b.set(variant, "Pattern", Variant::Enum(pattern));
        if base == METAL {
            b.set(variant, "MetalnessMap", content(GRID));
        }
        let x = -10.5 + 7.0 * i as f32;
        let part = b.part(BLOCK, [6.0, 6.0, 6.0], at([x, 3.0, 0.0]), colour, base);
        b.set(
            part,
            "MaterialVariantSerialized",
            Variant::String(name.to_owned()),
        );
    }
}

/// The grid `MaterialVariant` at 4 studs a tile on faces that are not a
/// whole number of tiles, and on two equal blocks a fraction of a tile
/// apart, which tells a grid anchored to the part from one anchored to the
/// world, and which corner (or the centre) of the face it starts at.
fn material_variant_anchor(b: &mut Builder) {
    b.block([2048.0, 16.0, 2048.0], [0.0, -8.0, 0.0], [91, 91, 91]);
    let variant = b.material_variant("Grid");
    b.set(variant, "BaseMaterial", Variant::Enum(PLASTIC));
    b.set(variant, "ColorMap", content(GRID));
    b.set(variant, "StudsPerTile", float(4.0));
    let blocks: [([f32; 3], [f32; 3]); 4] = [
        ([5.0, 5.0, 1.0], [-9.0, 2.5, 0.0]),
        ([5.0, 5.0, 1.0], [-2.75, 3.25, 0.0]),
        ([7.0, 3.0, 1.0], [4.5, 1.5, 0.0]),
        ([3.0, 7.0, 3.0], [11.0, 3.5, 0.0]),
    ];
    for (size, position) in blocks {
        let part = b.part(BLOCK, size, at(position), [248, 248, 248], PLASTIC);
        b.set(
            part,
            "MaterialVariantSerialized",
            Variant::String("Grid".to_owned()),
        );
    }
}

/// `Enum.AlphaMode`.
const OVERLAY: u32 = 0;
const TRANSPARENCY: u32 = 1;

/// A lathed cone of our own, 4 x 5 x 4 studs: a round base flaring into a
/// tapering body with a cap, smooth normals, U around (0 and 1 at -Z) and V
/// down from the tip.
fn cone() -> Generated {
    const SEGMENTS: u32 = 32;
    const RINGS: u32 = 16;
    // Radius and height up the profile, bottom edge to tip.
    let profile = |t: f32| -> (f32, f32) {
        let y = -2.5 + 5.0 * t;
        let r = if t < 0.06 {
            2.0
        } else {
            0.35 + 1.45 * (1.0 - (t - 0.06) / 0.94).powf(1.4)
        };
        (r, y)
    };
    let mut mesh = Generated {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        triangles: Vec::new(),
    };
    let around = |segment: u32| {
        let u = segment as f32 / SEGMENTS as f32;
        let (sin, cos) = (u * std::f32::consts::TAU).sin_cos();
        (u, sin, cos)
    };
    for ring in 0..=RINGS {
        let t = ring as f32 / RINGS as f32;
        let (r, y) = profile(t);
        let (r0, y0) = profile((t - 0.01).max(0.0));
        let (r1, y1) = profile((t + 0.01).min(1.0));
        // The outward normal of the profile's tangent.
        let (dr, dy) = (r1 - r0, y1 - y0);
        let length = (dr * dr + dy * dy).sqrt();
        let (nr, ny) = (dy / length, -dr / length);
        for segment in 0..=SEGMENTS {
            let (u, sin, cos) = around(segment);
            mesh.positions.push([r * sin, y, -r * cos]);
            mesh.normals.push([nr * sin, ny, -nr * cos]);
            mesh.uvs.push([u, 1.0 - t]);
        }
    }
    let row = SEGMENTS + 1;
    for ring in 0..RINGS {
        for segment in 0..SEGMENTS {
            let a = ring * row + segment;
            let (b, c, d) = (a + 1, a + row, a + row + 1);
            mesh.triangles.push([a, c, b]);
            mesh.triangles.push([b, c, d]);
        }
    }
    // Flat caps, the base facing down and the tip's facing up.
    for (t, ny) in [(0.0, -1.0), (1.0, 1.0)] {
        let (r, y) = profile(t);
        let centre = mesh.positions.len() as u32;
        mesh.positions.push([0.0, y, 0.0]);
        mesh.normals.push([0.0, ny, 0.0]);
        mesh.uvs.push([0.5, 1.0 - t]);
        for segment in 0..=SEGMENTS {
            let (u, sin, cos) = around(segment);
            mesh.positions.push([r * sin, y, -r * cos]);
            mesh.normals.push([0.0, ny, 0.0]);
            mesh.uvs.push([u, 1.0 - t]);
        }
        for segment in 0..SEGMENTS {
            let (a, b) = (centre + 1 + segment, centre + 2 + segment);
            mesh.triangles.push(if ny < 0.0 {
                [centre, a, b]
            } else {
                [centre, b, a]
            });
        }
    }
    mesh
}

/// The cone's colour map, 64 x 64: orange with two white bands, a dark
/// band along the bottom rows and a blue stripe down the left columns, so a
/// flipped or turned mapping shows.
fn cone_texture() -> GeneratedImage {
    const SIDE: u32 = 64;
    let mut rgba = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let colour = match (x, y) {
                (0..=3, _) => [40, 90, 220],
                (_, 56..) => [40, 40, 40],
                (_, 16..=23) | (_, 36..=43) => [245, 245, 245],
                _ => [240, 120, 20],
            };
            rgba.extend(colour);
            rgba.push(255);
        }
    }
    GeneratedImage {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The cone as a `MeshPart`: with its own texture, then wearing
/// `SurfaceAppearance`s with the grid as their colour map, `AlphaMode`
/// Overlay tinted by `Color`, `AlphaMode` Transparency, and the grid as
/// metalness and roughness maps too.
fn surface_appearances(b: &mut Builder) {
    b.block([2048.0, 16.0, 2048.0], [0.0, -8.0, 0.0], [91, 91, 91]);
    let mesh = cone();
    b.generated_mesh_part(
        &mesh,
        Some(&cone_texture()),
        [-9.0, 2.5, 0.0],
        [163, 162, 165],
    );
    let looks: [(u32, [u8; 3], bool); 3] = [
        (OVERLAY, [255, 170, 0], false),
        (TRANSPARENCY, [255, 255, 255], false),
        (OVERLAY, [255, 255, 255], true),
    ];
    for (i, (mode, [r, g, bl], maps)) in looks.into_iter().enumerate() {
        let x = -3.0 + 6.0 * i as f32;
        let part = b.generated_mesh_part(&mesh, None, [x, 2.5, 0.0], [196, 40, 28]);
        let look = b.child(part, "SurfaceAppearance");
        b.set(look, "ColorMap", content(GRID));
        b.set(look, "AlphaMode", Variant::Enum(mode));
        b.set(look, "Color", rgb(r, g, bl));
        if maps {
            b.set(look, "MetalnessMap", content(GRID));
            b.set(look, "RoughnessMap", content(GRID));
        }
    }
}

pub(super) fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "template_spawn",
            about: "The Baseplate template's textured floor and SpawnLocation decal, as Play opens",
            eye: [0.0, 8.0, 16.0],
            look_at: [0.0, 1.0, 0.0],
            fov: 70.0,
            lighting: studio(),
            probes: vec![
                ("spawn centre", [0.0, 1.0, 0.0]),
                ("spawn edge", [5.5, 1.0, 5.5]),
                ("spawn side", [0.0, 0.5, 6.0]),
                ("floor near", [-6.0, 0.0, 9.0]),
                ("floor far", [-20.0, 0.0, -40.0]),
                ("sky", [0.0, 300.0, -2000.0]),
            ],
            build: |b| {
                template_world(b);
                template_effects(b);
            },
        },
        Scene {
            name: "noob",
            about: "The classic noob up close: colours, round head and the default face",
            eye: [0.0, 4.5, -8.0],
            look_at: [0.0, 3.0, 0.0],
            fov: 50.0,
            lighting: studio(),
            probes: vec![
                ("face", [0.0, 4.5, -0.65]),
                ("head side", [0.55, 4.7, -0.5]),
                ("torso", [0.0, 3.0, -0.5]),
                ("left arm", [-1.5, 3.0, -0.5]),
                ("right leg", [0.5, 1.0, -0.5]),
            ],
            build: |b| {
                b.floor();
                noob(b, 0.0);
            },
        },
        Scene {
            name: "decals_textures",
            about: "A stretched Decal, a Texture tiled at 2 studs with an offset, a tinted half transparent Texture",
            eye: [0.0, 4.0, -14.0],
            look_at: [0.0, 3.0, 0.0],
            fov: 70.0,
            lighting: studio(),
            probes: vec![
                ("decal", [-7.0, 3.0, -0.5]),
                ("tiled", [0.0, 3.0, -0.5]),
                ("tinted", [7.0, 3.0, -0.5]),
                ("floor", [0.0, 0.0, -6.0]),
            ],
            build: decals,
        },
        Scene {
            name: "material_variants",
            about: "MaterialVariants with a colour map: StudsPerTile 4, Organic at 8, tinted, and on Metal with a metalness map",
            eye: [0.0, 9.0, -16.0],
            look_at: [0.0, 3.0, 0.0],
            fov: 70.0,
            lighting: studio(),
            probes: vec![
                ("grid front", [-10.5, 3.0, -3.0]),
                ("organic front", [-3.5, 3.0, -3.0]),
                ("tinted front", [3.5, 3.0, -3.0]),
                ("metallic front", [10.5, 3.0, -3.0]),
                ("grid top", [-10.5, 6.0, 0.0]),
                ("metallic top", [10.5, 6.0, 0.0]),
            ],
            build: material_variants,
        },
        Scene {
            name: "texture_blend",
            about: "Blue tinted and untinted Textures at Transparency 0, 0.25, 0.5 and 0.75 over red parts",
            eye: [0.0, 6.5, -22.0],
            look_at: [0.0, 6.5, 0.0],
            fov: 70.0,
            lighting: studio(),
            probes: [0.0, 0.25, 0.5, 0.75]
                .into_iter()
                .enumerate()
                .flat_map(|(i, _)| {
                    let x = -10.5 + 7.0 * i as f32;
                    [("blue", [x, 10.0, -0.5]), ("white", [x, 3.0, -0.5])]
                })
                .collect(),
            build: texture_blend,
        },
        Scene {
            name: "material_variant_anchor",
            about: "The grid MaterialVariant at 4 studs on 5, 7 and 3 stud faces, two equal blocks a fraction of a tile apart",
            eye: [0.0, 4.0, -18.0],
            look_at: [1.0, 3.0, 0.0],
            fov: 70.0,
            lighting: studio(),
            probes: vec![
                ("five", [-9.0, 2.5, -0.5]),
                ("five shifted", [-2.75, 3.25, -0.5]),
                ("seven", [4.5, 1.5, -0.5]),
                ("three", [11.0, 3.5, -1.5]),
            ],
            build: material_variant_anchor,
        },
        Scene {
            name: "surface_appearance",
            about: "A MeshPart with its TextureID, and SurfaceAppearances: Overlay tinted, Transparency, metalness and roughness maps",
            eye: [0.0, 5.0, -15.0],
            look_at: [0.0, 2.5, 0.0],
            fov: 70.0,
            lighting: studio(),
            probes: vec![
                ("texture", [-9.0, 1.5, -1.5]),
                ("overlay", [-3.0, 1.5, -1.5]),
                ("transparency", [3.0, 1.5, -1.5]),
                ("maps", [9.0, 1.5, -1.5]),
            ],
            build: surface_appearances,
        },
    ]
}
