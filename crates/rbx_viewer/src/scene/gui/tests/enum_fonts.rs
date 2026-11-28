//! Every `Enum.Font` member down to the family it draws in: never the
//! default face by accident, and always a family the client's own fonts
//! package carries, so no member silently drops to a system font.

use std::collections::HashMap;

use rbx_assets::{AssetCache, AssetRef, AssetResolver, MemoryFetcher, NativeContent};

use super::*;
use crate::fonts::{Face, Family};

/// The family each member draws in: the `FontFace` Studio 0.748 gives it
/// (see `rbx_dom`'s own walk of the enum), with `Arial.json` and
/// `GothamSSm.json` drawn in their replacements.
const DRAWN: [(u32, &str, &str); 52] = [
    (0, "Legacy", "LegacyArial"),
    (1, "Arial", "Arimo"),
    (2, "ArialBold", "Arimo"),
    (3, "SourceSans", "SourceSansPro"),
    (4, "SourceSansBold", "SourceSansPro"),
    (5, "SourceSansLight", "SourceSansPro"),
    (6, "SourceSansItalic", "SourceSansPro"),
    (7, "Bodoni", "AccanthisADFStd"),
    (8, "Garamond", "Guru"),
    (9, "Cartoon", "ComicNeueAngular"),
    (10, "Code", "Inconsolata"),
    (11, "Highway", "HighwayGothic"),
    (12, "SciFi", "Zekton"),
    (13, "Arcade", "PressStart2P"),
    (14, "Fantasy", "Balthazar"),
    (15, "Antique", "RomanAntique"),
    (16, "SourceSansSemibold", "SourceSansPro"),
    (17, "Gotham", "Montserrat"),
    (18, "GothamMedium", "Montserrat"),
    (19, "GothamBold", "Montserrat"),
    (20, "GothamBlack", "Montserrat"),
    (21, "AmaticSC", "AmaticSC"),
    (22, "Bangers", "Bangers"),
    (23, "Creepster", "Creepster"),
    (24, "DenkOne", "DenkOne"),
    (25, "Fondamento", "Fondamento"),
    (26, "FredokaOne", "FredokaOne"),
    (27, "GrenzeGotisch", "GrenzeGotisch"),
    (28, "IndieFlower", "IndieFlower"),
    (29, "JosefinSans", "JosefinSans"),
    (30, "Jura", "Jura"),
    (31, "Kalam", "Kalam"),
    (32, "LuckiestGuy", "LuckiestGuy"),
    (33, "Merriweather", "Merriweather"),
    (34, "Michroma", "Michroma"),
    (35, "Nunito", "Nunito"),
    (36, "Oswald", "Oswald"),
    (37, "PatrickHand", "PatrickHand"),
    (38, "PermanentMarker", "PermanentMarker"),
    (39, "Roboto", "Roboto"),
    (40, "RobotoCondensed", "RobotoCondensed"),
    (41, "RobotoMono", "RobotoMono"),
    (42, "Sarpanch", "Sarpanch"),
    (43, "SpecialElite", "SpecialElite"),
    (44, "TitilliumWeb", "TitilliumWeb"),
    (45, "Ubuntu", "Ubuntu"),
    (46, "BuilderSans", "BuilderSans"),
    (47, "BuilderSansMedium", "BuilderSans"),
    (48, "BuilderSansBold", "BuilderSans"),
    (49, "BuilderSansExtraBold", "BuilderSans"),
    (50, "Arimo", "Arimo"),
    (51, "ArimoBold", "Arimo"),
];

/// The families in Studio 0.748's `content-fonts.zip`
/// (`version-8d7a1009c17e4ff8`), the package every `rbxasset://fonts/` path
/// is fetched from.
const SHIPPED: [&str; 43] = [
    "AccanthisADFStd",
    "AmaticSC",
    "Arimo",
    "Balthazar",
    "Bangers",
    "BuilderExtended",
    "BuilderMono",
    "BuilderSans",
    "ComicNeueAngular",
    "Creepster",
    "DenkOne",
    "Fondamento",
    "FredokaOne",
    "GrenzeGotisch",
    "Guru",
    "HighwayGothic",
    "Inconsolata",
    "IndieFlower",
    "JosefinSans",
    "Jura",
    "Kalam",
    "LegacyArial",
    "LegacyArimo",
    "LuckiestGuy",
    "Merriweather",
    "Michroma",
    "Montserrat",
    "NotoSansCJKFallback",
    "Nunito",
    "Oswald",
    "PatrickHand",
    "PermanentMarker",
    "PressStart2P",
    "Roboto",
    "RobotoCondensed",
    "RobotoMono",
    "RomanAntique",
    "Sarpanch",
    "SourceSansPro",
    "SpecialElite",
    "TitilliumWeb",
    "Ubuntu",
    "Zekton",
];

/// The face a label with `Font` set to `ordinal` draws in.
fn drawn(ordinal: u32) -> Face {
    let (mut dom, gui) = screen_gui();
    let label = dom.new_instance("TextLabel", "TextLabel", Some(gui));
    dom.set_property(label, "Text", Variant::String("x".into()))
        .unwrap();
    dom.set_property(label, "Font", Variant::Enum(ordinal))
        .unwrap();
    resolve(&screens(&dom), VIEWPORT)
        .remove(0)
        .text
        .expect("a TextLabel carries text")
        .text
        .face
}

#[test]
fn every_enum_font_draws_in_its_own_shipped_family() {
    for (ordinal, name, family) in DRAWN {
        let face = drawn(ordinal);
        assert_eq!(
            face.family,
            Face::named(family, 400, false).family,
            "Enum.Font.{name}"
        );
        assert!(
            SHIPPED.contains(&family),
            "Enum.Font.{name}: {family} is not in the package"
        );
    }
}

/// Fetches every drawn family and the face it picks from Roblox's CDN, and
/// checks each family picks a face file of its own that parses as a font:
/// two families landing on the same file would be one silently standing in
/// for the other. The one pair that does share is Roblox's own doing: the
/// package's `LegacyArial.json` names `Arimo-Regular.ttf` too. Network, so
/// ignored by default.
#[test]
#[ignore = "downloads Studio's content-fonts.zip from setup.rbxcdn.com"]
fn every_enum_font_resolves_to_a_distinct_typeface_on_the_cdn() {
    let dir = std::env::temp_dir().join(format!("rbx_viewer_enum_fonts_{}", std::process::id()));
    let cache = AssetCache::new(Some(dir.clone())).unwrap();
    let native = NativeContent::new(cache.native_packages_dir());
    let resolver = AssetResolver::new(cache, Box::new(MemoryFetcher::new()), native);
    let mut files: HashMap<AssetRef, &str> = HashMap::new();
    for (ordinal, name, family) in DRAWN {
        let face = drawn(ordinal);
        let json = resolver
            .resolve(&face.family)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let entry = Family::parse(&json.bytes)
            .and_then(|parsed| parsed.closest(face.weight, face.italic).cloned())
            .unwrap_or_else(|| panic!("Enum.Font.{name}: {family}.json lists no face"));
        assert!(
            matches!(entry.asset, AssetRef::Native(_)),
            "Enum.Font.{name}: {:?} is not in the package",
            entry.asset
        );
        let bytes = resolver
            .resolve(&entry.asset)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(bytes.bytes);
        assert!(
            db.faces().next().is_some(),
            "Enum.Font.{name}: the face does not parse"
        );
        let family = if family == "LegacyArial" {
            "Arimo"
        } else {
            family
        };
        let owner = *files.entry(entry.asset.clone()).or_insert(family);
        assert_eq!(
            owner, family,
            "Enum.Font.{name}: {:?} is also {owner}'s",
            entry.asset
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}
