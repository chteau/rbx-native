//! The rasterised glyphs every text quad samples, packed into one RGBA image:
//! white with the glyph's coverage in alpha, so the GUI shader tints it with
//! the vertex colour exactly as it tints the flat-white texel a background
//! samples. The GPU copy lives in `renderer::gui::atlas` at slot `GLYPHS`.

use std::collections::HashMap;

use cosmic_text::{CacheKey, SwashContent, SwashImage};

const INITIAL_SIDE: u32 = 512;
/// ponytail: a glyph that will not fit a full atlas this size is dropped
/// rather than the atlas paged; a second texture, or evicting glyphs no
/// element uses any more, is the upgrade if a place ever gets there.
const MAX_SIDE: u32 = 4096;
/// Empty texels around every glyph, so linear sampling at its edge never
/// bleeds a neighbour in.
const PADDING: u32 = 1;

/// One glyph's place in the atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::renderer::gui) struct Glyph {
    pub(in crate::renderer::gui) x: u32,
    pub(in crate::renderer::gui) y: u32,
    pub(in crate::renderer::gui) width: u32,
    pub(in crate::renderer::gui) height: u32,
    /// Where the bitmap sits against the glyph's origin, in swash's own
    /// convention: `left` to the right of it, `top` *above* the baseline.
    pub(in crate::renderer::gui) left: i32,
    pub(in crate::renderer::gui) top: i32,
    /// A colour bitmap (an emoji), which Roblox draws untinted.
    pub(in crate::renderer::gui) color: bool,
}

/// A row of glyphs sharing one height: the simplest packer that never has to
/// move what it already placed.
struct Shelf {
    y: u32,
    height: u32,
    /// Where the next glyph on this shelf starts.
    x: u32,
}

pub(in crate::renderer::gui) struct GlyphAtlas {
    side: u32,
    pixels: Vec<u8>,
    shelves: Vec<Shelf>,
    /// `None` for a glyph with no bitmap — a space, or one that no longer
    /// fits — so it is not rasterised again on every build.
    glyphs: HashMap<CacheKey, Option<Glyph>>,
    dirty: bool,
    /// Bumped every time the atlas is grown and repacked: every `Glyph` handed
    /// out before that is stale, and a build that saw the bump starts over.
    generation: u32,
}

impl GlyphAtlas {
    pub(super) fn new() -> Self {
        GlyphAtlas {
            side: INITIAL_SIDE,
            pixels: vec![0; (INITIAL_SIDE * INITIAL_SIDE * 4) as usize],
            shelves: Vec::new(),
            glyphs: HashMap::new(),
            dirty: false,
            generation: 0,
        }
    }

    pub(in crate::renderer::gui) fn side(&self) -> u32 {
        self.side
    }

    pub(in crate::renderer::gui) fn generation(&self) -> u32 {
        self.generation
    }

    /// `Some(None)` for a glyph known to have no bitmap, `None` for one never
    /// rasterised.
    pub(super) fn get(&self, key: &CacheKey) -> Option<Option<Glyph>> {
        self.glyphs.get(key).copied()
    }

    /// Packs `image` in, growing the atlas when it is full.
    pub(super) fn insert(&mut self, key: CacheKey, image: Option<&SwashImage>) -> Option<Glyph> {
        let placed = image.and_then(|image| self.place(image));
        self.glyphs.insert(key, placed);
        placed
    }

    fn place(&mut self, image: &SwashImage) -> Option<Glyph> {
        let placement = image.placement;
        if placement.width == 0 || placement.height == 0 {
            return None;
        }
        // Subpixel masks only come out of a rendering mode nothing here asks
        // for; treating one as coverage would draw its colour fringes.
        if image.content == SwashContent::SubpixelMask {
            return None;
        }
        let (x, y) = loop {
            if let Some(corner) = self.pack(placement.width + PADDING, placement.height + PADDING) {
                break corner;
            }
            if self.side >= MAX_SIDE {
                return None;
            }
            self.grow();
        };

        let color = image.content == SwashContent::Color;
        for row in 0..placement.height {
            for column in 0..placement.width {
                let at = (((y + row) * self.side + x + column) * 4) as usize;
                let from = (row * placement.width + column) as usize;
                self.pixels[at..at + 4].copy_from_slice(&match color {
                    true => image.data[from * 4..from * 4 + 4]
                        .try_into()
                        .unwrap_or([0; 4]),
                    false => [255, 255, 255, image.data[from]],
                });
            }
        }
        self.dirty = true;
        Some(Glyph {
            x,
            y,
            width: placement.width,
            height: placement.height,
            left: placement.left,
            top: placement.top,
            color,
        })
    }

    /// The top-left corner of a free `width` by `height` box, or `None` when
    /// no shelf has room and there is none left to open.
    fn pack(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width > self.side {
            return None;
        }
        // A shelf up to half again as tall as the glyph: taller wastes the
        // difference under every glyph on it.
        let fits = |shelf: &Shelf| {
            shelf.height >= height
                && shelf.height <= height + height / 2
                && shelf.x + width <= self.side
        };
        if let Some(shelf) = self.shelves.iter_mut().find(|shelf| fits(shelf)) {
            let corner = (shelf.x, shelf.y);
            shelf.x += width;
            return Some(corner);
        }
        let y = self
            .shelves
            .last()
            .map_or(0, |shelf| shelf.y + shelf.height);
        if y + height > self.side {
            return None;
        }
        self.shelves.push(Shelf {
            y,
            height,
            x: width,
        });
        Some((0, y))
    }

    /// Doubles the side and starts over: the glyphs are re-rasterised into
    /// the new atlas by the build that notices the generation change, which
    /// is a copy out of the swash cache each, not a rasterisation.
    fn grow(&mut self) {
        self.side = (self.side * 2).min(MAX_SIDE);
        self.pixels = vec![0; (self.side * self.side * 4) as usize];
        self.shelves.clear();
        self.glyphs.clear();
        self.generation += 1;
        self.dirty = true;
    }

    /// The whole image when anything changed since the last call. Whole
    /// rather than the changed rows: a GUI's text is rasterised once, and a
    /// few megabytes uploaded once is not worth tracking dirty rectangles for.
    pub(in crate::renderer::gui) fn take_dirty(&mut self) -> Option<(u32, &[u8])> {
        match std::mem::take(&mut self.dirty) {
            true => Some((self.side, &self.pixels)),
            false => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_text::{CacheKeyFlags, FontSystem, Placement, Weight};

    /// A key over whichever face the system has first: the atlas never
    /// looks inside a key, but `fontdb::ID` has no constructor of its own.
    /// `None` on a machine with no fonts at all, where the tests are skipped.
    fn keys() -> Option<impl Fn(u16) -> CacheKey> {
        let id = FontSystem::new().db().faces().next()?.id;
        Some(move |glyph| {
            CacheKey::new(
                id,
                glyph,
                16.0,
                (0.0, 0.0),
                Weight::NORMAL,
                CacheKeyFlags::empty(),
            )
            .0
        })
    }

    fn mask(width: u32, height: u32) -> SwashImage {
        SwashImage {
            content: SwashContent::Mask,
            placement: Placement {
                left: 1,
                top: 12,
                width,
                height,
            },
            data: vec![200; (width * height) as usize],
            ..SwashImage::default()
        }
    }

    #[test]
    fn a_mask_lands_as_white_with_its_coverage_in_alpha() {
        let Some(key) = keys() else {
            return;
        };
        let mut atlas = GlyphAtlas::new();

        let glyph = atlas.insert(key(1), Some(&mask(3, 2))).unwrap();

        assert_eq!((glyph.x, glyph.y, glyph.width, glyph.height), (0, 0, 3, 2));
        assert_eq!((glyph.left, glyph.top), (1, 12));
        assert!(!glyph.color);
        let (side, pixels) = atlas.take_dirty().unwrap();
        assert_eq!(side, INITIAL_SIDE);
        assert_eq!(&pixels[0..4], &[255, 255, 255, 200]);
        // The padding column after the glyph stays clear.
        assert_eq!(&pixels[12..16], &[0, 0, 0, 0]);
        assert!(atlas.take_dirty().is_none(), "nothing changed since");
        assert_eq!(atlas.get(&key(1)), Some(Some(glyph)));
    }

    #[test]
    fn glyphs_of_one_height_share_a_shelf_and_a_taller_one_opens_another() {
        let Some(key) = keys() else {
            return;
        };
        let mut atlas = GlyphAtlas::new();

        let first = atlas.insert(key(1), Some(&mask(4, 4))).unwrap();
        let second = atlas.insert(key(2), Some(&mask(4, 4))).unwrap();
        let tall = atlas.insert(key(3), Some(&mask(4, 40))).unwrap();

        assert_eq!((first.x, first.y), (0, 0));
        assert_eq!((second.x, second.y), (5, 0));
        assert_eq!((tall.x, tall.y), (0, 5));
    }

    #[test]
    fn an_empty_glyph_is_remembered_as_having_no_bitmap() {
        let Some(key) = keys() else {
            return;
        };
        let mut atlas = GlyphAtlas::new();

        assert!(atlas.insert(key(1), Some(&mask(0, 0))).is_none());
        assert!(atlas.insert(key(2), None).is_none());

        assert_eq!(atlas.get(&key(1)), Some(None));
        assert_eq!(atlas.get(&key(2)), Some(None));
        assert_eq!(atlas.get(&key(3)), None);
    }

    #[test]
    fn a_full_atlas_grows_and_forgets_what_it_held() {
        let Some(key) = keys() else {
            return;
        };
        let mut atlas = GlyphAtlas::new();
        let big = INITIAL_SIDE - PADDING;
        atlas.insert(key(1), Some(&mask(big, big))).unwrap();
        assert_eq!(atlas.generation(), 0);

        let placed = atlas.insert(key(2), Some(&mask(big, big))).unwrap();

        assert_eq!(atlas.generation(), 1);
        assert_eq!(atlas.side(), INITIAL_SIDE * 2);
        assert_eq!((placed.x, placed.y), (0, 0));
        assert_eq!(
            atlas.get(&key(1)),
            None,
            "the first glyph has to be rasterised again"
        );
    }
}
