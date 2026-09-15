//! The published texture packs, transcribed from Roblox's own material
//! reference: one row per `Enum.Material` value, in the order that page lists
//! them.
//!
//! Ids are the four maps in `MapKind::ALL` order (colour, normal, metalness,
//! roughness), 0 standing for a map that material has none of. Only the tiling
//! scale is ours: Roblox publishes no studs-per-tile, so anything not measured
//! against Studio by eye keeps [`DEFAULT_STUDS_PER_TILE`].

use crate::{Flavor, Material, DEFAULT_STUDS_PER_TILE};

// Local alias purely so a row stays on one line.
const TILE: f32 = DEFAULT_STUDS_PER_TILE;

const fn pack(
    name: &'static str,
    studs_per_tile: f32,
    color: u64,
    normal: u64,
    metalness: u64,
    roughness: u64,
) -> Material {
    Material {
        name,
        flavor: Flavor::Solid,
        studs_per_tile,
        color,
        normal,
        metalness,
        roughness,
    }
}

/// A material Roblox ships no texture pack for; its tiling scale never reaches
/// a sampler, so it keeps the default.
const fn procedural(name: &'static str, flavor: Flavor) -> Material {
    Material {
        name,
        flavor,
        studs_per_tile: TILE,
        color: 0,
        normal: 0,
        metalness: 0,
        roughness: 0,
    }
}

impl Material {
    const fn flavored(self, flavor: Flavor) -> Self {
        Material { flavor, ..self }
    }
}

/// The 2022 set: every `Enum.Material` a `BasePart` can carry, `Air` and
/// `Water` included — those two are legal on a part and Roblox draws them as
/// plain plastic, so they are rows here rather than a special case upstream.
pub(crate) const CURRENT: [Material; 45] = [
    pack("Asphalt", TILE, 9930003046, 9429449876, 0, 9429450346),
    pack("Basalt", TILE, 9920482056, 9438412214, 0, 9438412457),
    pack("Brick", 5.0, 9920482813, 9438453152, 0, 9438453413),
    pack("Cardboard", TILE, 14108651729, 14108654002, 0, 14108654299),
    pack("Carpet", TILE, 14108662587, 14108663154, 0, 14108663726),
    pack(
        "CeramicTiles",
        TILE,
        17429425079,
        17429425915,
        17429426100,
        17429426861,
    ),
    pack(
        "ClayRoofTiles",
        TILE,
        18147681935,
        18147683410,
        0,
        18147684855,
    ),
    pack("Cobblestone", TILE, 9919718991, 9438457162, 0, 9438457470),
    pack("Concrete", 12.0, 9920484153, 9466554006, 0, 9466554186),
    pack(
        "CorrodedMetal",
        TILE,
        9920589327,
        9439548484,
        9439548749,
        9439556441,
    ),
    pack("CrackedLava", TILE, 9920484943, 9438508790, 0, 9438509046),
    pack(
        "DiamondPlate",
        TILE,
        10237720195,
        9438583222,
        9438583347,
        9438583558,
    ),
    pack("Fabric", TILE, 9920517696, 9873280412, 0, 9873282563),
    pack("Foil", TILE, 9466552117, 9424786192, 9424786272, 9424786620),
    procedural("ForceField", Flavor::ForceField),
    pack("Glacier", TILE, 9920518732, 9438812958, 0, 9438851286),
    pack("Glass", TILE, 9438868521, 7547304785, 0, 7547304892).flavored(Flavor::Glass),
    pack("Granite", TILE, 9920550238, 9438882935, 0, 9438883109),
    pack("Grass", 4.0, 9920551868, 9438955773, 0, 9438955997),
    pack("Ground", TILE, 9920554482, 9439043558, 0, 9439043765),
    pack("Ice", TILE, 9920555943, 9467301039, 0, 9467301203),
    pack("LeafyGrass", TILE, 9920557906, 9439080781, 0, 9439080950),
    pack("Leather", TILE, 14108670073, 14108670486, 0, 14108670748),
    pack("Limestone", TILE, 9920561437, 9439415191, 0, 9439415495),
    pack("Marble", TILE, 9439430596, 9439431240, 0, 9439431383),
    pack("Metal", 4.0, 9920574687, 9873295432, 9873318201, 9873318890),
    pack("Mud", TILE, 9920578473, 9439509827, 0, 9439510012),
    procedural("Neon", Flavor::Neon),
    pack("Pavement", TILE, 9920579943, 9439519281, 0, 9439519532),
    pack("Pebble", TILE, 9920581082, 9439528644, 0, 9439537267),
    pack("Plaster", TILE, 14108671255, 14108671870, 0, 14108672378),
    // The one material Roblox publishes a lone normal map for: `Plastic` has
    // the stud-like relief of its own pack (from the Modern texture pack) with
    // no colour, metalness or roughness map at all, so a plastic part keeps its
    // `Color` and only gains the bumps. `SmoothPlastic` is the same material
    // without even that.
    pack("Plastic", TILE, 0, 9475362634, 0, 0),
    pack("Rock", TILE, 9920587470, 9439538417, 0, 9439545859),
    pack(
        "RoofShingles",
        TILE,
        119722544879522,
        77534750680073,
        0,
        129397260312247,
    ),
    pack(
        "Rubber",
        TILE,
        14108673018,
        14108674698,
        14108674894,
        14108675142,
    ),
    pack("Salt", TILE, 9920590225, 9439565809, 0, 9439566688),
    pack("Sand", 6.0, 9920591683, 9439577084, 0, 9439577327),
    pack("Sandstone", TILE, 9920596120, 9439596530, 0, 9439596711),
    pack("Slate", 6.0, 9920599782, 9439612514, 0, 9439612733),
    procedural("SmoothPlastic", Flavor::Solid),
    pack("Snow", TILE, 9920620284, 9439632006, 0, 9439632145),
    pack("Wood", 4.0, 9920625290, 9439641376, 0, 9439648605),
    pack("WoodPlanks", 8.0, 9920626778, 9439650689, 0, 9439658127),
    procedural("Air", Flavor::Solid),
    procedural("Water", Flavor::Solid),
];

/// The pre-2022 set, which only covers 18 materials; [`crate::material`] falls
/// back to [`CURRENT`] for the rest.
pub(crate) const LEGACY: [Material; 18] = [
    pack("Brick", 5.0, 7546648254, 7546649654, 0, 7546650017),
    pack("Cobblestone", TILE, 7546651802, 7546652689, 0, 7546652892),
    pack("Concrete", 12.0, 7546653328, 7546653707, 0, 7546653868),
    pack(
        "CorrodedMetal",
        TILE,
        7547183598,
        7547181182,
        7547184321,
        7547184588,
    ),
    pack(
        "DiamondPlate",
        TILE,
        7546654401,
        7546654536,
        7547162002,
        7547162137,
    ),
    pack("Fabric", TILE, 7547100606, 7547100915, 0, 7547101072),
    pack("Foil", TILE, 7546644642, 7546644903, 7546644642, 7546644963),
    pack("Glass", TILE, 7547304577, 7547304785, 0, 7547304892).flavored(Flavor::Glass),
    pack("Granite", TILE, 7547164400, 7546654648, 0, 7547164660),
    pack("Grass", 4.0, 7547167347, 7547168653, 0, 7547169207),
    pack("Ice", TILE, 7546644642, 7547171198, 0, 7547171276),
    pack("Marble", TILE, 7547174345, 7547176060, 0, 7547177213),
    pack("Metal", 4.0, 7547178395, 7547287997, 7547288112, 7547179082),
    pack("Pebble", TILE, 7547291174, 7546645052, 0, 7547291306),
    pack("Sand", 6.0, 7547294684, 7547294810, 0, 7547295087),
    pack("Slate", 6.0, 7547297050, 7547297808, 0, 7547298051),
    pack("Wood", 4.0, 7547190453, 7547190548, 7547190619, 7547303147),
    pack(
        "WoodPlanks",
        8.0,
        7547301709,
        7547188159,
        7547188891,
        7547332869,
    ),
];
