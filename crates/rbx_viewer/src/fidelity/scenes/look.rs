//! Scenes of what a player sees first in a new place: the Baseplate
//! template's textured floor and spawn, the noob, and `Decal`/`Texture`
//! children drawn on parts.

use rbx_dom::{Variant, Vector3Data};

use super::super::{at, content, float, rgb, Builder, Scene, BLOCK, PLASTIC};
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
    ]
}
