//! This project's own class icons — flat, multi-colour SVGs shipped in
//! `assets/icons/default/dark` and `assets/icons/default/light` (spec'd in
//! `assets/icons/README.md`, gitignored working notes, not a shipped asset)
//! — replacing the sprite sheet Roblox's own Studio ships, which this
//! project no longer downloads or draws.
//!
//! Rasterized ourselves with `resvg` rather than painted through GPUI's own
//! `svg()` element: that element is a monochrome icon renderer (it always
//! recolors its SVG to one flat `text_color`, discarding whatever fill the
//! file itself carries — fine for Lucide's single-color glyphs, wrong for
//! this kit's palette), so the sliced-sprite path the old Roblox sheet used
//! (`render_image::to_render_image`, painted with `img()`) is kept and fed
//! from a rasterized SVG instead of a downloaded PNG tile.
//!
//! Both variants are embedded at compile time; [`IconPack`] (an editor
//! setting, see `settings::Settings::icon_pack`) only picks which one
//! [`icon_tile`] reads from — no rebuild needed to switch. A user's own pack
//! (`crate::packs`) is layered over either: [`set_user_pack`] installs it, and
//! whatever it leaves out is still drawn from the built-in kit.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use gpui_kit::RenderImage;
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::packs::IconOverlay;
use crate::render_image::to_render_image;

mod slugs;
mod tint;

use slugs::CLASS_ICON_SLUGS;
pub(crate) use tint::tint;

/// Every SVG in `assets/icons/default/dark`, embedded at compile time.
#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets/icons/default/dark"]
struct DefaultIcons;

/// Every SVG in `assets/icons/default/light`, embedded at compile time
/// alongside [`DefaultIcons`] — [`IconPack`] picks between the two at
/// lookup time, so both ship in the binary regardless of which is active.
#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets/icons/default/light"]
struct LightIcons;

/// Which of the kit's two equal-sized variants is currently drawn — a
/// persisted editor setting (see `settings::Settings::icon_pack`), not a
/// build-time choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum IconPack {
    #[default]
    Dark,
    Light,
}

/// Every icon in the kit is authored on a 16x16 `viewBox` (see
/// `assets/icons/README.md`'s "Canvas" section); rasterized at 2x for a
/// sharp downscale to the Explorer's `CLASS_ICON_SIZE`.
const RENDER_SIZE: u32 = 32;

/// The rasterized icon for `class` from the requested `pack`, or `None` for
/// a class the icon kit doesn't cover (falls back to a Lucide glyph — see
/// `explorer::resolve_icon`) or whose SVG failed to parse (a build-time
/// invariant, not a runtime one: every file under both `assets/icons/default`
/// variants is checked by this module's own tests).
pub(crate) fn icon_tile(class: &str, pack: IconPack) -> Option<Arc<RenderImage>> {
    if let Some(hit) = TILE_CACHE
        .read()
        .ok()
        .and_then(|cache| cache.get(pack, class))
    {
        return hit;
    }
    let installed = USER_PACK.read().ok().and_then(|pack| pack.clone());
    let tile = icon_tile_over(class, pack, installed.as_deref());
    if let Ok(mut cache) = TILE_CACHE.write() {
        cache.insert(pack, class, tile.clone());
    }
    tile
}

/// Every tile resolved so far, keyed by the pack it was read from.
///
/// [`rasterize`] parses an SVG and renders a pixmap on every call, which is
/// fine once per class per Explorer rebuild but not once per row per frame —
/// the insert picker (see `shell::explorer_edit::picker`) lists hundreds of
/// classes and rebuilds its list on every keystroke. The `None`s are cached
/// too: a class the kit does not cover is the *common* case there, and
/// re-deciding it would re-walk `CLASS_ICON_SLUGS` each time.
///
/// Keyed by *class* rather than by slug, which does mean two classes
/// sharing a tile each rasterize it once: an installed pack is allowed to
/// cover a class the kit does not (see [`IconOverlay::svg`]), so the class
/// is the only key that stays correct once one is layered on.
///
/// Bounded by the table itself — at most one entry per mapped class per
/// variant, a 32x32 RGBA tile each — so it needs no eviction.
/// [`set_user_pack`] is the only thing that can invalidate it, and clears
/// it.
static TILE_CACHE: RwLock<TileCache> = RwLock::new(TileCache::new());

/// [`icon_tile`] against an explicit overlay rather than the installed one, so
/// the precedence can be tested without touching process-wide state.
fn icon_tile_over(
    class: &str,
    pack: IconPack,
    overlay: Option<&IconOverlay>,
) -> Option<Arc<RenderImage>> {
    let slug = CLASS_ICON_SLUGS
        .iter()
        .find(|(name, _)| *name == class)
        .map(|(_, slug)| *slug);

    // The installed pack goes first, and falls through on a drawing that will
    // not parse rather than blanking the icon: it is somebody else's file.
    if let Some(image) = overlay
        .and_then(|overlay| overlay.svg(class, slug))
        .and_then(|svg| rasterize(&svg))
    {
        return Some(image);
    }

    let slug = slug?;
    let file = match pack {
        IconPack::Dark => DefaultIcons::get(&format!("{slug}.svg")),
        IconPack::Light => LightIcons::get(&format!("{slug}.svg")),
    }?;
    rasterize(&file.data)
}

/// The user's installed icon pack, drawn over the built-in kit — see
/// `crate::packs`. Process-wide because the Explorer resolves icons deep
/// inside row construction with no handle to the editor's state; set once at
/// startup and again whenever the Explorer's menu picks another.
static USER_PACK: RwLock<Option<Arc<IconOverlay>>> = RwLock::new(None);

/// Installs `pack` as the overlay, or removes it with `None`. The caller
/// rebuilds whatever already resolved an icon.
pub(crate) fn set_user_pack(pack: Option<IconOverlay>) {
    if let Ok(mut slot) = USER_PACK.write() {
        *slot = pack.map(Arc::new);
    }
    // Every cached tile was resolved against the overlay that just went
    // away, including the misses — a pack that covers a class the kit does
    // not would otherwise stay invisible until the next launch.
    if let Ok(mut cache) = TILE_CACHE.write() {
        cache.clear();
    }
}

/// [`TILE_CACHE`]'s map: one class-keyed map per variant, rather than one
/// map keyed by the pair, so a lookup borrows the class name instead of
/// allocating a `String` to build a tuple key with.
struct TileCache {
    dark: Option<HashMap<String, Option<Arc<RenderImage>>>>,
    light: Option<HashMap<String, Option<Arc<RenderImage>>>>,
}

impl TileCache {
    const fn new() -> Self {
        TileCache {
            dark: None,
            light: None,
        }
    }

    fn of(&self, pack: IconPack) -> &Option<HashMap<String, Option<Arc<RenderImage>>>> {
        match pack {
            IconPack::Dark => &self.dark,
            IconPack::Light => &self.light,
        }
    }

    /// `Some(hit)` only when this class has been resolved before — the outer
    /// `Option` is "have we looked", the inner one "does the kit cover it".
    fn get(&self, pack: IconPack, class: &str) -> Option<Option<Arc<RenderImage>>> {
        self.of(pack).as_ref()?.get(class).cloned()
    }

    fn insert(&mut self, pack: IconPack, class: &str, tile: Option<Arc<RenderImage>>) {
        let slot = match pack {
            IconPack::Dark => &mut self.dark,
            IconPack::Light => &mut self.light,
        };
        slot.get_or_insert_with(HashMap::new)
            .insert(class.to_owned(), tile);
    }

    fn clear(&mut self) {
        self.dark = None;
        self.light = None;
    }
}

/// Renders `svg` to a square RGBA tile.
///
/// The kit is authored on a 16x16 canvas, but the scale is taken from the
/// document's own size so a pack drawn on 24x24 or 32x32 fills the tile
/// instead of being cropped to its top-left corner, and a drawing that is not
/// square is centred on the shorter axis rather than pinned to the top-left.
/// (`usvg` refuses a document with no size, so the divisor is never zero.)
///
/// `pub(crate)`: also `action_icons`'s own rasterizer, for the ribbon's
/// action-icon kit (`assets/icons/actions`) — same 16x16 canvas, same
/// premultiplied-alpha fixup, no reason for a second copy of either.
pub(crate) fn rasterize(svg: &[u8]) -> Option<Arc<RenderImage>> {
    let tree = Tree::from_data(svg, &Options::default()).ok()?;
    let mut pixmap = Pixmap::new(RENDER_SIZE, RENDER_SIZE)?;
    let size = tree.size();
    let tile = RENDER_SIZE as f32;
    let scale = tile / size.width().max(size.height());
    let (x, y) = (
        (tile - size.width() * scale) / 2.0,
        (tile - size.height() * scale) / 2.0,
    );
    resvg::render(
        &tree,
        Transform::from_row(scale, 0.0, 0.0, scale, x, y),
        &mut pixmap.as_mut(),
    );

    // `Pixmap` is premultiplied alpha; `to_render_image`'s consumers (decoded
    // PNG tiles, wgpu readback frames) are not, and the anti-aliased edges
    // every glyph here has would come out darkened without this.
    let straight: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let demultiplied = pixel.demultiply();
            [
                demultiplied.red(),
                demultiplied.green(),
                demultiplied.blue(),
                demultiplied.alpha(),
            ]
        })
        .collect();

    to_render_image(straight, RENDER_SIZE, RENDER_SIZE)
}

#[cfg(test)]
#[path = "class_icons/tests.rs"]
mod tests;
