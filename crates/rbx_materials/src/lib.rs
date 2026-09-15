//! The texture packs behind `Enum.Material`, as Roblox publishes them.
//!
//! Roblox ships every built-in material as four public assets (colour, normal,
//! metalness, roughness maps); this crate is that table and nothing else — no
//! network, no GPU, no DOM. Start at [`material`], which answers what one
//! `Enum.Material` name is made of in a given [`Set`].
//!
//! Terrain has its own per-face packs which are deliberately left out: the
//! viewer draws no terrain.

mod table;

/// Which generation of texture packs a place asks for, through
/// `MaterialService.Use2022MaterialsXml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Set {
    /// The 2022 materials, i.e. `Use2022MaterialsXml = true`.
    Current,
    /// The pre-2022 materials. Only 18 of them exist; anything else falls back
    /// to its [`Set::Current`] row, which is also what Studio shows.
    Legacy,
}

/// How a material shades beyond its texture maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// Lit normally. Covers every material with a texture pack, and the
    /// untextured ones (`SmoothPlastic`, `Air`, `Water`) that are simply the
    /// part's own colour — as is `Plastic`, whose lone normal map adds relief
    /// to that colour without replacing it.
    Solid,
    /// Unlit and emissive.
    Neon,
    /// Translucent whatever the part's `Transparency` says.
    ForceField,
    /// Textured, but far more reflective than its roughness map alone implies.
    Glass,
}

/// One of the four maps a material can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapKind {
    Color,
    Normal,
    Metalness,
    Roughness,
}

/// `MaterialVariant.StudsPerTile`'s own default, and ours for every material
/// Roblox does not publish a tiling scale for (it publishes none of them).
pub const DEFAULT_STUDS_PER_TILE: f32 = 10.0;

/// One built-in material: its texture pack and how it shades.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    name: &'static str,
    flavor: Flavor,
    studs_per_tile: f32,
    // 0 rather than `Option<u64>`: no Roblox asset has id 0, and it keeps a row
    // of the table to one readable line.
    color: u64,
    normal: u64,
    metalness: u64,
    roughness: u64,
}

impl MapKind {
    pub const ALL: [MapKind; 4] = [
        MapKind::Color,
        MapKind::Normal,
        MapKind::Metalness,
        MapKind::Roughness,
    ];

    /// Where this kind sits in [`MapKind::ALL`], which is the order every
    /// four-slot map array in this workspace is laid out in.
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl Material {
    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn flavor(&self) -> Flavor {
        self.flavor
    }

    pub fn studs_per_tile(&self) -> f32 {
        self.studs_per_tile
    }

    /// The asset id of one map, or `None` where this material has no such map —
    /// which is most of them for [`MapKind::Metalness`]: only `Metal`,
    /// `DiamondPlate`, `CorrodedMetal`, `Foil`, `CeramicTiles` and `Rubber` are
    /// metals in the 2022 set, everything else being a dielectric.
    pub fn map(&self, kind: MapKind) -> Option<u64> {
        let id = match kind {
            MapKind::Color => self.color,
            MapKind::Normal => self.normal,
            MapKind::Metalness => self.metalness,
            MapKind::Roughness => self.roughness,
        };
        (id != 0).then_some(id)
    }

    /// Whether Roblox publishes no texture pack at all for this material, which
    /// leaves a renderer to draw it from the part's `Color` and its [`Flavor`].
    ///
    /// Not the same question as "is this material drawn as plain plastic":
    /// `Plastic` has a normal map and no colour map, so it is shaded from its
    /// own `Color` like a bare material while still sampling that one map.
    pub fn is_procedural(&self) -> bool {
        MapKind::ALL.iter().all(|&kind| self.map(kind).is_none())
    }
}

/// The material an `Enum.Material` name stands for, or `None` for a name the
/// API dump does not have (a newer Studio's, say).
pub fn material(name: &str, set: Set) -> Option<&'static Material> {
    let legacy = match set {
        Set::Current => None,
        Set::Legacy => find(&table::LEGACY, name),
    };
    legacy.or_else(|| find(&table::CURRENT, name))
}

fn find(rows: &'static [Material], name: &str) -> Option<&'static Material> {
    rows.iter().find(|material| material.name == name)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
