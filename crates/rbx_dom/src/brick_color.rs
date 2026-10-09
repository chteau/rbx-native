//! Roblox's `BrickColor` table: the 208 named colours, each with its number,
//! its RGB, and — for 128 of them — its place in the palette Studio's colour
//! picker shows. Transcribed from creator-docs'
//! `reference/engine/datatypes/BrickColor.yaml`, so that everything that
//! names a `BrickColor` (the Properties panel, a script) agrees on it.

mod table;

use table::TABLE;

/// One named colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrickColor {
    /// What `BrickColor.new(number)` and a file's `BrickColor` value hold.
    pub number: u32,
    pub name: &'static str,
    /// Zero-based index into the 128-colour palette, for the colours that
    /// are in it (`BrickColor.palette(index)`).
    pub palette: Option<u8>,
    pub rgb: [u8; 3],
}

/// "Medium stone grey": what `BrickColor.new` returns for a number or name
/// the table does not have, per the docs, and a new part's colour.
pub const DEFAULT_NUMBER: u32 = 194;

impl BrickColor {
    pub fn from_number(number: u32) -> Option<&'static BrickColor> {
        TABLE.iter().find(|color| color.number == number)
    }

    /// The exact name, as the table spells it.
    pub fn from_name(name: &str) -> Option<&'static BrickColor> {
        TABLE.iter().find(|color| color.name == name)
    }

    pub fn from_palette(index: u8) -> Option<&'static BrickColor> {
        TABLE.iter().find(|color| color.palette == Some(index))
    }

    /// The closest colour to `rgb`, the way `BrickColor.new(Color3)` and a
    /// part's `BrickColor` pick it: the smallest sum of absolute per-channel
    /// differences, over every colour in the table. The docs do not say
    /// which of two equally close colours wins; the first in table order
    /// does here.
    pub fn nearest(rgb: [u8; 3]) -> &'static BrickColor {
        TABLE
            .iter()
            .min_by_key(|color| {
                color
                    .rgb
                    .iter()
                    .zip(rgb)
                    .map(|(a, b)| a.abs_diff(b) as u32)
                    .sum::<u32>()
            })
            .expect("the table is not empty")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_whole() {
        assert_eq!(TABLE.len(), 208);
        let palette: Vec<u8> = (0..128)
            .filter_map(BrickColor::from_palette)
            .filter_map(|c| c.palette)
            .collect();
        assert_eq!(palette, (0..128).collect::<Vec<u8>>());
    }

    #[test]
    fn every_id_resolves_to_its_own_entry() {
        for color in TABLE {
            assert_eq!(BrickColor::from_number(color.number), Some(color));
        }
        let mut numbers: Vec<u32> = TABLE.iter().map(|c| c.number).collect();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), TABLE.len(), "duplicate numbers");
    }

    #[test]
    fn avatar_body_colour_ids_resolve_to_the_expected_rgb() {
        for (id, name, rgb) in [
            (1, "White", [242, 243, 243]),
            (21, "Bright red", [196, 40, 28]),
            (23, "Bright blue", [13, 105, 172]),
            (24, "Bright yellow", [245, 205, 48]),
            (26, "Black", [27, 42, 53]),
            (102, "Medium blue", [110, 153, 202]),
            (119, "Br. yellowish green", [164, 189, 71]),
            (194, "Medium stone grey", [163, 162, 165]),
            (1001, "Institutional white", [248, 248, 248]),
        ] {
            let color = BrickColor::from_number(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!((color.name, color.rgb), (name, rgb), "{id}");
        }
    }

    #[test]
    fn entries_match_the_docs() {
        let grey = BrickColor::from_number(DEFAULT_NUMBER).unwrap();
        assert_eq!(grey.name, "Medium stone grey");
        assert_eq!(grey.rgb, [163, 162, 165]);
        assert_eq!(BrickColor::from_name("Pastel Blue").unwrap().number, 11);
        assert_eq!(BrickColor::from_palette(70).unwrap().name, "Gold");
        assert_eq!(BrickColor::from_number(1032).unwrap().name, "Hot pink");
        assert_eq!(BrickColor::from_number(9999), None);
    }

    #[test]
    fn nearest_is_the_smallest_total_channel_distance() {
        assert_eq!(BrickColor::nearest([163, 162, 165]).number, 194);
        assert_eq!(BrickColor::nearest([255, 0, 0]).name, "Really red");
        // Off by a little from "Bright red" (196, 40, 28).
        assert_eq!(BrickColor::nearest([200, 42, 30]).name, "Bright red");
    }
}
