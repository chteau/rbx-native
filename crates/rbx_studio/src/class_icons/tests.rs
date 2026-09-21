use super::*;

/// Every slug this module names must actually be a file both
/// `DefaultIcons` and `LightIcons` embed, and every embedded file must
/// parse and rasterize — a build-time invariant on the (gitignored,
/// hand-authored) icon kit, checked here so a bad SVG fails `cargo test`
/// rather than silently blanking an icon.
#[test]
fn every_mapped_slug_rasterizes_in_both_packs() {
    for (class, slug) in CLASS_ICON_SLUGS {
        for pack in [IconPack::Dark, IconPack::Light] {
            assert!(
                icon_tile(class, pack).is_some(),
                "{slug}.svg ({class}) failed to rasterize in {pack:?}"
            );
        }
    }
}

/// The whole point of `IconPack`: the same class looks up a different
/// file, and thus different pixels, depending on which pack is asked
/// for — `part.svg` deliberately uses different fill colours between the
/// `dark` and `light` folders (see `assets/icons/default/*/part.svg`).
/// The slug a class draws, or `None` when the kit does not cover it —
/// the table lookup on its own, without rasterizing anything.
fn slug_of(class: &str) -> Option<&'static str> {
    CLASS_ICON_SLUGS
        .iter()
        .find(|(name, _)| *name == class)
        .map(|(_, slug)| *slug)
}

#[test]
fn a_class_outside_roblox_metadata_reuses_its_family_tile() {
    // Roblox's own `ExplorerImageIndex` covers none of the left-hand
    // classes, which is why the insert picker used to list them with a
    // bare glyph. Each is pointed at the tile its family already has
    // rather than at a drawing of its own.
    for (outsider, family) in [
        ("FileMesh", "BlockMesh"),
        ("DynamicMesh", "BlockMesh"),
        ("KeyframeSequence", "Animation"),
        ("Vector3Curve", "Animation"),
        ("BinaryStringValue", "StringValue"),
        ("Motor", "Motor6D"),
        ("PartOperation", "UnionOperation"),
        ("VehicleController", "Humanoid"),
    ] {
        let slug = slug_of(outsider).unwrap_or_else(|| panic!("{outsider} has no tile"));
        assert_eq!(
            Some(slug),
            slug_of(family),
            "{outsider} should share {family}'s tile"
        );
    }
}

#[test]
fn a_tile_authored_beyond_the_sheet_is_claimed_by_exactly_its_own_classes() {
    // The five drawings added past Roblox's 147: each exists because no
    // tile in the kit fitted, so each has to be claimed by something,
    // and nothing else may quietly pick it up.
    for (slug, classes) in [
        ("style-sheet", &["StyleBase", "StyleSheet"][..]),
        ("style-rule", &["StyleRule"][..]),
        ("style-link", &["StyleDerive", "StyleLink"][..]),
        ("intersect-operation", &["IntersectOperation"][..]),
        ("body-colors", &["BodyColors"][..]),
    ] {
        let mut claimed: Vec<&str> = CLASS_ICON_SLUGS
            .iter()
            .filter(|(_, s)| *s == slug)
            .map(|(class, _)| *class)
            .collect();
        claimed.sort_unstable();
        assert_eq!(claimed, classes, "{slug}");
    }
}

#[test]
fn a_resolved_tile_is_served_from_the_cache_next_time() {
    // `rasterize` is far too expensive to run once per picker row per
    // frame; the second lookup must hand back the very same image.
    let first = icon_tile("Part", IconPack::Dark).expect("Part is covered by the icon kit");
    let second = icon_tile("Part", IconPack::Dark).expect("Part is covered by the icon kit");
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn clearing_the_cache_forgets_both_variants() {
    let mut cache = TileCache::new();
    cache.insert(IconPack::Dark, "Part", None);
    cache.insert(IconPack::Light, "Part", None);
    assert!(cache.get(IconPack::Dark, "Part").is_some());
    assert!(cache.get(IconPack::Light, "Part").is_some());

    cache.clear();
    assert!(cache.get(IconPack::Dark, "Part").is_none());
    assert!(cache.get(IconPack::Light, "Part").is_none());
}

#[test]
fn a_class_the_kit_does_not_cover_is_remembered_as_a_miss() {
    // The misses are the common case in the picker — every row for a
    // class outside the kit — so re-walking the table for each one is
    // exactly what the cache is there to stop.
    let mut cache = TileCache::new();
    assert!(cache.get(IconPack::Dark, "NotARealClass").is_none());
    cache.insert(IconPack::Dark, "NotARealClass", None);
    assert!(matches!(
        cache.get(IconPack::Dark, "NotARealClass"),
        Some(None)
    ));
}

#[test]
fn icon_tile_reads_from_the_requested_pack() {
    let dark = icon_tile("Part", IconPack::Dark).expect("Part is covered by the icon kit");
    let light = icon_tile("Part", IconPack::Light).expect("Part is covered by the icon kit");
    assert_ne!(dark.as_bytes(0), light.as_bytes(0));
}

const RED_SQUARE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
    <rect width="24" height="24" fill="#ff0000"/></svg>"##;

/// The pack is an overlay over the kit: its drawing wins for a class it
/// names, and every other class is still the kit's.
#[test]
fn an_installed_pack_wins_for_the_classes_it_names_and_leaves_the_rest() {
    let overlay = IconOverlay::with("Part", RED_SQUARE);

    let mine = icon_tile_over("Part", IconPack::Dark, Some(&overlay)).unwrap();
    let kit = icon_tile_over("Part", IconPack::Dark, None).unwrap();
    assert_ne!(mine.as_bytes(0), kit.as_bytes(0));

    let untouched = icon_tile_over("Folder", IconPack::Dark, Some(&overlay)).unwrap();
    let folder = icon_tile_over("Folder", IconPack::Dark, None).unwrap();
    assert_eq!(untouched.as_bytes(0), folder.as_bytes(0));
}

/// A pack can name a class the kit has no tile for at all, and its icon is
/// drawn where the kit alone would have fallen back to a glyph.
#[test]
fn a_pack_can_cover_a_class_the_kit_does_not() {
    let overlay = IconOverlay::with("NotARealClass", RED_SQUARE);
    assert!(icon_tile_over("NotARealClass", IconPack::Dark, None).is_none());
    assert!(icon_tile_over("NotARealClass", IconPack::Dark, Some(&overlay)).is_some());
}

/// A drawing on a 24x24 canvas fills the tile the same way a 16x16 one
/// does, rather than being cropped to its top-left two thirds: the
/// far corner pixel of a full-bleed square is opaque either way.
#[test]
fn a_larger_canvas_is_scaled_to_fill_the_tile() {
    let overlay = IconOverlay::with("Part", RED_SQUARE);
    let image = icon_tile_over("Part", IconPack::Dark, Some(&overlay)).unwrap();
    let bytes = image.as_bytes(0).unwrap();
    let last_pixel = &bytes[bytes.len() - 4..];
    assert_eq!(last_pixel[3], 255, "bottom-right corner must be opaque");
}

/// A 32x16 drawing sits in the middle of the tile, not its top half: the
/// first row is empty and the middle one is not.
#[test]
fn a_drawing_that_is_not_square_is_centred() {
    let wide = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16">
        <rect width="32" height="16" fill="#00ff00"/></svg>"##;
    let overlay = IconOverlay::with("Part", wide);
    let image = icon_tile_over("Part", IconPack::Dark, Some(&overlay)).unwrap();
    let bytes = image.as_bytes(0).unwrap();
    let alpha = |row: usize, column: usize| bytes[(row * RENDER_SIZE as usize + column) * 4 + 3];

    assert_eq!(alpha(0, 16), 0, "the top row is padding");
    assert_eq!(alpha(31, 16), 0, "so is the bottom row");
    assert_eq!(alpha(16, 16), 255, "the drawing is in the middle");
}

/// A document with no size cannot be scaled to anything; it is refused,
/// and so falls through to the kit, rather than dividing by zero.
#[test]
fn a_drawing_with_no_size_is_refused() {
    let empty = br#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"/>"#;
    assert!(rasterize(empty).is_none());
}

/// A pack file that will not parse falls through to the kit rather than
/// blanking the icon.
#[test]
fn a_broken_drawing_falls_back_to_the_kit() {
    let overlay = IconOverlay::with("Part", b"this is not svg");
    let shown = icon_tile_over("Part", IconPack::Dark, Some(&overlay)).unwrap();
    let kit = icon_tile_over("Part", IconPack::Dark, None).unwrap();
    assert_eq!(shown.as_bytes(0), kit.as_bytes(0));
}
