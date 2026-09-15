//! Real Roblox Studio class icons, sliced out of `ClassImages.PNG` — the same
//! 16x16-per-tile sprite sheet Studio's own Explorer draws from.
//!
//! The `ClassName -> tile index` table is Roblox's own `ExplorerImageIndex`
//! metadata. It ships in `ReflectionMetadata.xml` rather than in a content
//! package, so it is scanned out of the tracker's mirror
//! (<https://raw.githubusercontent.com/MaximumADHD/Roblox-Client-Tracker/roblox/ReflectionMetadata.xml>,
//! already credited in this project's `README.md`) at runtime and cached
//! locally, exactly like `ClassImages.PNG` itself: nothing derived from
//! Roblox's own files is embedded in the binary or committed to this
//! repository.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use gpui_kit::RenderImage;
use rbx_assets::{decode_image, AssetCache, AssetRef, AssetResolver, MemoryFetcher, NativeContent};

use crate::render_image::to_render_image;

const TILE_SIZE: u32 = 16;
const SHEET_PATH: &str = "textures/ClassImages.PNG";
const REFLECTION_METADATA_URL: &str = "https://raw.githubusercontent.com/MaximumADHD/Roblox-Client-Tracker/roblox/ReflectionMetadata.xml";
/// Cached alongside `ClassImages.PNG` (via [`AssetCache::get_native`] /
/// [`AssetCache::put_native`]) under its own subdirectory, so the distilled
/// index is never mistaken for a raw content-package path.
const CLASS_ICONS_CACHE_PATH: &str = "reflection/class_icons.json";

static CLASS_ICONS: LazyLock<HashMap<String, u16>> = LazyLock::new(load_class_icons);

/// The decoded `ClassImages.PNG` sprite sheet, kept as plain RGBA8 so a tile
/// can be sliced out of it on demand.
pub(crate) struct SpriteSheet {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

/// Downloads and decodes the sprite sheet through the same native-content
/// pipeline `rbx_viewer` uses for `rbxasset://` textures.
///
/// `None` on any failure (no network, a changed CDN layout, ...): every class
/// then falls back to its Lucide stand-in rather than the whole place
/// refusing to load, matching how a missing per-asset texture is already
/// handled.
pub(crate) fn load_sheet() -> Option<SpriteSheet> {
    let cache = AssetCache::new(None).ok()?;
    let native = NativeContent::new(cache.native_packages_dir());
    // Only ever asked to resolve a native path below; a real network fetcher
    // is not reachable from here and not needed for it.
    let resolver = AssetResolver::new(cache, Box::new(MemoryFetcher::new()), native);

    let reference = AssetRef::Native(SHEET_PATH.to_string());
    let asset = match resolver.resolve(&reference) {
        Ok(asset) => asset,
        Err(err) => {
            eprintln!("rbxstudio: no class icons ({err})");
            return None;
        }
    };
    let decoded = match decode_image(&asset) {
        Ok(decoded) => decoded,
        Err(err) => {
            eprintln!("rbxstudio: no class icons ({err})");
            return None;
        }
    };

    let (width, height) = decoded.dimensions();
    Some(SpriteSheet {
        pixels: decoded.into_raw(),
        width,
        height,
    })
}

/// Loads the `ClassName -> ExplorerImageIndex` table from the local cache,
/// fetching and distilling Roblox's metadata mirror on a cache miss.
///
/// Empty on any failure (no network, a changed file layout, a cache write
/// error, ...): every class then falls back to its Lucide stand-in, exactly
/// like [`load_sheet`] already does for the sprite sheet itself.
fn load_class_icons() -> HashMap<String, u16> {
    let cache = match AssetCache::new(None) {
        Ok(cache) => cache,
        Err(err) => {
            eprintln!("rbxstudio: no class icons ({err})");
            return HashMap::new();
        }
    };

    if let Some(bytes) = cache.get_native(CLASS_ICONS_CACHE_PATH) {
        match serde_json::from_slice(&bytes) {
            Ok(table) => return table,
            // A stale/corrupt cache entry shouldn't strand the user without
            // icons forever — fall through and refetch instead.
            Err(err) => eprintln!("rbxstudio: cached class icons unreadable ({err}), refetching"),
        }
    }

    let xml = match http_get_text(REFLECTION_METADATA_URL) {
        Ok(xml) => xml,
        Err(err) => {
            eprintln!("rbxstudio: no class icons ({err})");
            return HashMap::new();
        }
    };

    let table = extract_class_icons(&xml);
    match serde_json::to_vec(&table) {
        Ok(bytes) => {
            if let Err(err) = cache.put_native(CLASS_ICONS_CACHE_PATH, &bytes) {
                eprintln!("rbxstudio: could not cache class icons ({err})");
            }
        }
        Err(err) => eprintln!("rbxstudio: could not serialize class icons ({err})"),
    }
    table
}

/// Mirrors `rbx_assets::native`'s own `http_get_text`: this crate needs its
/// own GET because the metadata mirror is a plain GitHub raw URL, not a
/// `rbxasset://` reference `AssetResolver` knows how to resolve.
fn http_get_text(url: &str) -> Result<String, String> {
    let mut response = ureq::get(url).call().map_err(|err| err.to_string())?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|err| err.to_string())
}

/// Scans `xml` for every `ReflectionMetadataClass` block and pulls out its
/// `Name`/`ExplorerImageIndex` pair, skipping blocks missing either one.
///
/// Property order inside a block is not guaranteed, and most blocks nest
/// further `Item`s (per-member metadata) after their own properties — see
/// [`find_matching_close_tag`] for why the block boundary can't just be the
/// first `</Item>` encountered.
fn extract_class_icons(xml: &str) -> HashMap<String, u16> {
    const BLOCK_MARKER: &str = "<Item class=\"ReflectionMetadataClass\">";

    let mut icons = HashMap::new();
    let mut cursor = 0;
    while let Some(offset) = xml[cursor..].find(BLOCK_MARKER) {
        let block_start = cursor + offset + BLOCK_MARKER.len();
        let Some(block_end) = find_matching_close_tag(xml, block_start) else {
            break; // Unbalanced tail: nothing more to safely parse.
        };
        let block = &xml[block_start..block_end];
        let name = extract_string_property(block, "Name");
        let index = extract_string_property(block, "ExplorerImageIndex")
            .and_then(|raw| raw.parse::<u16>().ok());
        if let (Some(name), Some(index)) = (name, index) {
            icons.insert(name, index);
        }
        cursor = block_end;
    }
    icons
}

/// Finds the `</Item>` that closes the `<Item>` opened just before `from`,
/// by tracking nested opens/closes instead of assuming the first `</Item>`
/// seen is the one that matches — class blocks routinely nest further
/// `<Item>`s (member metadata) before their own closing tag.
fn find_matching_close_tag(xml: &str, from: usize) -> Option<usize> {
    let mut depth = 1u32;
    let mut pos = from;
    loop {
        let next_open = xml[pos..].find("<Item").map(|i| pos + i);
        let next_close = xml[pos..].find("</Item>").map(|i| pos + i);
        match (next_open, next_close) {
            (Some(open), Some(close)) if open < close => {
                depth += 1;
                pos = open + "<Item".len();
            }
            (_, Some(close)) => {
                pos = close + "</Item>".len();
                depth -= 1;
                if depth == 0 {
                    return Some(pos);
                }
            }
            _ => return None,
        }
    }
}

/// Reads `<string name="{key}">value</string>` out of a block, wherever it
/// falls among the block's other properties.
fn extract_string_property(block: &str, key: &str) -> Option<String> {
    let marker = format!("<string name=\"{key}\">");
    let start = block.find(&marker)? + marker.len();
    let end = start + block[start..].find("</string>")?;
    Some(block[start..end].to_string())
}

/// The tile index Roblox documents for `class`, or `None` for a class the
/// mirrored metadata does not cover (undocumented, or newer than the mirror,
/// or unreachable this run).
pub(crate) fn tile_index(class: &str) -> Option<u16> {
    CLASS_ICONS.get(class).copied()
}

/// Slices tile `index` out of `sheet`, ready for GPUI to paint.
///
/// `None` if `index` is past the sheet's own tile count — only possible if
/// the distilled table and the downloaded sheet disagree (a future Studio
/// version adding tiles or classes independently of the other).
pub(crate) fn tile(sheet: &SpriteSheet, index: u16) -> Option<Arc<RenderImage>> {
    let tiles = sheet.width / TILE_SIZE;
    if u32::from(index) >= tiles {
        return None;
    }

    let x0 = u32::from(index) * TILE_SIZE;
    let stride = sheet.width as usize * 4;
    let mut pixels = Vec::with_capacity(TILE_SIZE as usize * TILE_SIZE as usize * 4);
    for y in 0..sheet.height.min(TILE_SIZE) {
        let row_start = y as usize * stride + x0 as usize * 4;
        let row_end = row_start + TILE_SIZE as usize * 4;
        pixels.extend_from_slice(&sheet.pixels[row_start..row_end]);
    }

    to_render_image(pixels, TILE_SIZE, TILE_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_name_and_index_when_both_present() {
        let xml = r#"
            <Item class="ReflectionMetadataClass">
              <Properties>
                <string name="Name">Actor</string>
                <string name="ExplorerImageIndex">113</string>
              </Properties>
            </Item>
        "#;
        assert_eq!(extract_class_icons(xml).get("Actor"), Some(&113));
    }

    #[test]
    fn skips_classes_missing_explorer_image_index() {
        let xml = r#"
            <Item class="ReflectionMetadataClass">
              <Properties>
                <string name="Name">Undocumented</string>
              </Properties>
            </Item>
        "#;
        assert_eq!(extract_class_icons(xml).get("Undocumented"), None);
    }

    #[test]
    fn property_order_within_a_block_does_not_matter() {
        let xml = r#"
            <Item class="ReflectionMetadataClass">
              <Properties>
                <string name="ExplorerImageIndex">19</string>
                <string name="Name">Workspace</string>
              </Properties>
            </Item>
        "#;
        assert_eq!(extract_class_icons(xml).get("Workspace"), Some(&19));
    }

    /// Mirrors the real file's shape for classes with members: a nested
    /// `<Item>` for per-member metadata, closing before the class block
    /// itself does. A naive "first `</Item>` wins" scan would truncate the
    /// class block early and, worse, misreport the next class's boundaries.
    #[test]
    fn nested_item_blocks_do_not_confuse_block_boundaries() {
        let xml = r#"
            <Item class="ReflectionMetadataClass">
              <Properties>
                <string name="Name">BindableFunction</string>
                <string name="ExplorerImageIndex">66</string>
              </Properties>
              <Item class="ReflectionMetadataYieldFunctions">
                <Item class="ReflectionMetadataMember">
                  <Properties>
                    <string name="Name">Invoke</string>
                  </Properties>
                </Item>
              </Item>
            </Item>
            <Item class="ReflectionMetadataClass">
              <Properties>
                <string name="Name">BindableEvent</string>
                <string name="ExplorerImageIndex">67</string>
              </Properties>
            </Item>
        "#;
        let icons = extract_class_icons(xml);
        assert_eq!(icons.get("BindableFunction"), Some(&66));
        assert_eq!(icons.get("BindableEvent"), Some(&67));
        assert_eq!(icons.get("Invoke"), None);
    }

    /// A hand-built two-tile sheet: solid red then solid blue, so slicing can
    /// be checked without a real download.
    fn two_tile_sheet() -> SpriteSheet {
        let mut pixels = Vec::new();
        for _ in 0..TILE_SIZE {
            for _ in 0..TILE_SIZE {
                pixels.extend_from_slice(&[255, 0, 0, 255]);
            }
            for _ in 0..TILE_SIZE {
                pixels.extend_from_slice(&[0, 0, 255, 255]);
            }
        }
        SpriteSheet {
            pixels,
            width: TILE_SIZE * 2,
            height: TILE_SIZE,
        }
    }

    #[test]
    fn slices_the_requested_tile_not_the_whole_sheet() {
        let sheet = two_tile_sheet();
        // BGRA-swapped by to_render_image: red (255,0,0) becomes (0,0,255).
        let red = tile(&sheet, 0).expect("first tile");
        assert_eq!(red.as_bytes(0).unwrap()[0..4], [0, 0, 255, 255]);
        let blue = tile(&sheet, 1).expect("second tile");
        assert_eq!(blue.as_bytes(0).unwrap()[0..4], [255, 0, 0, 255]);
    }

    #[test]
    fn an_index_past_the_sheet_has_no_tile() {
        let sheet = two_tile_sheet();
        assert!(tile(&sheet, 2).is_none());
    }
}
