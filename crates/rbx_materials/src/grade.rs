//! How far the real client's picture of each material sits from its
//! published colour map, as a regrade a renderer applies to that map once,
//! when it uploads it.
//!
//! The packs [`crate::material`] names are the published ones; the client
//! draws its own copies, and on a grey (163, 162, 165) slab under Studio's
//! template lighting some come out far brighter or darker, flatter or
//! busier than the published map gives (Asphalt at 0.37 of its brightness,
//! Basalt at 1.4). Fitted from the `materials_a`/`materials_b` fidelity
//! scenes: the mean of each slab's top face, and the spread of its
//! luminance, in the real client's frames against ours, both taken back
//! through the tone curve to light. Nothing of the client's own textures
//! is in here, only those two numbers per material.

/// A regrade of one colour map, in linear light: every texel pulled toward
/// (or pushed away from) the map's own mean by `contrast`, then scaled by
/// `gain` per channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grade {
    pub gain: [f32; 3],
    pub contrast: f32,
}

impl Grade {
    pub const IDENTITY: Grade = Grade {
        gain: [1.0; 3],
        contrast: 1.0,
    };

    /// `texel` (linear, one channel) regraded against `mean`, the map's
    /// mean in that channel.
    pub fn apply(&self, channel: usize, texel: f32, mean: f32) -> f32 {
        (self.gain[channel] * (mean + self.contrast * (texel - mean))).clamp(0.0, 1.0)
    }
}

const fn grade(name: &'static str, gain: [f32; 3], contrast: f32) -> (&'static str, Grade) {
    (name, Grade { gain, contrast })
}

/// One row per 2022 material the scenes measured; a material missing here is
/// drawn from its map as published.
const GRADES: [(&str, Grade); 37] = [
    grade("Asphalt", [0.329, 0.349, 0.341], 0.121),
    grade("Basalt", [1.407, 1.455, 1.433], 0.397),
    grade("Brick", [0.900, 1.071, 1.181], 0.712),
    grade("Cardboard", [0.947, 1.009, 1.023], 1.021),
    grade("Carpet", [1.275, 1.346, 1.336], 0.551),
    grade("CeramicTiles", [1.026, 1.066, 1.064], 0.830),
    grade("ClayRoofTiles", [0.866, 0.980, 1.012], 1.416),
    grade("Cobblestone", [0.998, 0.902, 0.779], 0.580),
    grade("Concrete", [0.869, 0.914, 0.925], 1.307),
    grade("CorrodedMetal", [0.795, 0.669, 0.534], 0.715),
    grade("CrackedLava", [1.581, 1.848, 1.899], 0.519),
    grade("DiamondPlate", [1.421, 1.210, 1.026], 1.539),
    grade("Fabric", [0.285, 0.306, 0.301], 2.500),
    grade("Foil", [0.756, 0.899, 0.946], 1.784),
    grade("Glacier", [1.365, 1.424, 1.370], 2.461),
    grade("Granite", [0.604, 0.627, 0.612], 1.368),
    grade("Grass", [0.727, 0.819, 0.811], 1.085),
    grade("Ground", [0.579, 0.636, 0.698], 0.540),
    grade("Ice", [1.194, 1.233, 1.213], 0.326),
    grade("LeafyGrass", [0.542, 0.552, 0.528], 0.162),
    grade("Leather", [1.045, 1.076, 1.067], 0.778),
    grade("Limestone", [1.086, 1.157, 1.188], 0.581),
    grade("Marble", [1.280, 1.298, 1.153], 0.442),
    grade("Metal", [3.000, 2.530, 1.470], 2.500),
    grade("Mud", [0.473, 0.505, 0.498], 0.409),
    grade("Pavement", [0.402, 0.436, 0.443], 0.210),
    grade("Pebble", [1.118, 1.191, 1.247], 0.805),
    grade("Plaster", [1.033, 1.075, 1.073], 0.687),
    grade("Rock", [0.462, 0.488, 0.483], 0.836),
    grade("RoofShingles", [0.888, 0.946, 0.963], 0.709),
    grade("Salt", [0.837, 0.866, 0.868], 1.518),
    grade("Sand", [1.076, 1.174, 1.301], 1.533),
    grade("Sandstone", [0.446, 0.480, 0.500], 0.724),
    grade("Slate", [0.688, 0.720, 0.707], 1.127),
    grade("Snow", [0.933, 0.951, 0.930], 0.934),
    grade("Wood", [1.089, 1.281, 1.304], 0.611),
    grade("WoodPlanks", [0.992, 1.023, 0.986], 0.536),
];

/// The regrade for the 2022 pack whose colour map is asset `color_map`, or
/// `None` for any other image (a `MaterialVariant`'s own map, a legacy pack).
pub fn grade_for(color_map: u64) -> Option<Grade> {
    let row = crate::table::CURRENT
        .iter()
        .find(|material| material.color == color_map)?;
    GRADES
        .iter()
        .find(|(name, _)| *name == row.name)
        .map(|(_, grade)| *grade)
}
