//! Shaping GUI text and rasterising its glyphs, with `cosmic-text`.
//!
//! One [`Typesetter`] serves the screen overlay and every in-world canvas: it
//! owns the font system (system fonts plus every Roblox face the loader has
//! landed), the swash rasteriser and the glyph atlas the quads sample. It is
//! also what the layout measures text with (see `scene::gui::TextMeasure`),
//! so `TextScaled` settles on the size the glyphs really take.

mod atlas;

use std::collections::{HashMap, HashSet};

use cosmic_text::{
    Align, Attrs, Buffer, CacheKey, Color, Ellipsize, Family, FontSystem, Metrics, Shaping, Style,
    SwashCache, UnderlineStyle, Weight, Wrap,
};
use rbx_assets::AssetRef;

pub(super) use atlas::{Glyph, GlyphAtlas};

use crate::fonts::{Face, Library};
use crate::scene::{gui_span_face, GuiText, GuiTextMeasure};

pub(super) struct Typesetter {
    system: FontSystem,
    cache: SwashCache,
    pub(super) atlas: GlyphAtlas,
    /// Family JSON reference → the family name fontdb filed its faces under,
    /// which is the name inside the face file, not the JSON's `name`.
    families: HashMap<AssetRef, String>,
    /// Every face file already handed to the font system.
    loaded: HashSet<AssetRef>,
}

impl Typesetter {
    /// Starts from the system's fonts: text in a family that has not landed
    /// yet — or never will, a face behind a cloud id with no API key — shapes
    /// in the locale's sans-serif rather than vanishing.
    pub(super) fn new() -> Self {
        Typesetter {
            system: FontSystem::new(),
            cache: SwashCache::new(),
            atlas: GlyphAtlas::new(),
            families: HashMap::new(),
            loaded: HashSet::new(),
        }
    }

    /// Loads every face in `wanted` whose bytes `library` has and this has
    /// not taken yet. `true` when anything was added, which is the caller's
    /// cue that text already laid out has to be shaped again.
    pub(super) fn adopt(&mut self, library: &Library, wanted: &[Face]) -> bool {
        let mut added = false;
        for face in wanted {
            let Some((asset, bytes)) = library.bytes_of(face) else {
                continue;
            };
            if !self.loaded.insert(asset.clone()) {
                continue;
            }
            let db = self.system.db_mut();
            let before = db.len();
            db.load_font_data(bytes.to_vec());
            // Matched by the name the face carries in its own tables, which
            // is only near the JSON's ("Gotham SSm" for `GothamSSm.json`):
            // the face just filed is asked for it.
            let name = db
                .faces()
                .nth(before)
                .and_then(|info| info.families.first())
                .map(|(name, _)| name.clone());
            if let Some(name) = name {
                self.families.insert(face.family.clone(), name);
                added = true;
            }
        }
        added
    }

    /// Whether `face`'s family has landed, for a test to check on.
    #[cfg(test)]
    pub(super) fn knows(&self, face: &Face) -> bool {
        self.families.contains_key(&face.family)
    }

    /// Shapes `text` at `size` pixels per line, wrapping at `width` where the
    /// text wraps at all, with every line aligned left: the quads align each
    /// line themselves, which is the only way an unwrapped line — one with no
    /// box width to align inside — can be centred.
    pub(super) fn shape(
        &mut self,
        text: &GuiText,
        size: f32,
        width: Option<f32>,
        ellipsize: Option<Ellipsize>,
    ) -> Buffer {
        let Typesetter {
            system, families, ..
        } = self;
        // A `<font size>` scales with `TextScaled` like the base size does.
        let scale = size / text.size.max(1.0);
        let mut buffer = Buffer::new(system, Metrics::new(size, size * text.line_height));
        buffer.set_size(width, None);
        buffer.set_wrap(match text.wrapped || text.scaled {
            true => Wrap::WordOrGlyph,
            false => Wrap::None,
        });
        if let Some(ellipsize) = ellipsize {
            buffer.set_ellipsize(ellipsize);
        }

        let base = attrs(families, &text.face);
        let spans: Vec<(&str, Attrs)> = text
            .spans
            .iter()
            .enumerate()
            .map(|(index, span)| {
                let mut attrs = attrs(families, &gui_span_face(&text.face, span)).metadata(index);
                if let Some(size) = span.size {
                    attrs =
                        attrs.metrics(Metrics::new(size * scale, size * scale * text.line_height));
                }
                // Only decorations read this colour (glyphs go by `metadata`,
                // which keeps the full-precision one): a byte per channel is
                // plenty for an underline.
                attrs = attrs.color(quantised(
                    span.color.unwrap_or(text.color),
                    span.alpha.unwrap_or(text.alpha),
                ));
                if span.underline {
                    attrs.text_decoration.underline = UnderlineStyle::Single;
                }
                attrs.text_decoration.strikethrough = span.strike;
                (span.text.as_str(), attrs)
            })
            .collect();
        buffer.set_rich_text(spans, &base, Shaping::Advanced, Some(Align::Left));
        buffer.shape_until_scroll(system, false);
        buffer
    }

    /// The glyph's place in the atlas, rasterising it on first sight; `None`
    /// for one with no bitmap.
    pub(super) fn glyph(&mut self, key: CacheKey) -> Option<Glyph> {
        if let Some(known) = self.atlas.get(&key) {
            return known;
        }
        let image = self.cache.get_image(&mut self.system, key).as_ref();
        self.atlas.insert(key, image)
    }
}

impl GuiTextMeasure for Typesetter {
    fn measure(&mut self, text: &GuiText, size: f32, max_width: Option<f32>) -> [f32; 2] {
        bounds(&self.shape(text, size, max_width, None))
    }
}

/// The widest line and the height of every line.
pub(super) fn bounds(buffer: &Buffer) -> [f32; 2] {
    buffer.layout_runs().fold([0.0f32, 0.0], |bounds, run| {
        [bounds[0].max(run.line_w), bounds[1] + run.line_height]
    })
}

/// The attributes one face shapes with: its family by the name fontdb knows
/// it under, or the sans-serif fallback while that is unknown.
fn attrs<'a>(families: &'a HashMap<AssetRef, String>, face: &Face) -> Attrs<'a> {
    Attrs::new()
        .family(
            families
                .get(&face.family)
                .map_or(Family::SansSerif, |name| Family::Name(name)),
        )
        .weight(Weight(face.weight))
        .style(match face.italic {
            true => Style::Italic,
            false => Style::Normal,
        })
}

fn quantised(color: [f32; 3], alpha: f32) -> Color {
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::rgba(byte(color[0]), byte(color[1]), byte(color[2]), byte(alpha))
}

/// A decoration's colour back out of the byte it was quantised to.
pub(super) fn unquantised(color: Color) -> ([f32; 3], f32) {
    let channel = |byte: u8| f32::from(byte) / 255.0;
    (
        [channel(color.r()), channel(color.g()), channel(color.b())],
        channel(color.a()),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::fonts::{Entry, Family};

    // A face file that will not parse — a truncated download, a cloud asset
    // that turned out not to be a font — must not be taken as the family: its
    // text keeps shaping in the fallback, and the file is not tried again.
    #[test]
    fn a_face_that_is_not_a_font_is_never_adopted() {
        let face = Face::named("Shelf", 400, false);
        let asset = AssetRef::Native("fonts/Shelf-Regular.ttf".to_string());
        let mut library = Library::default();
        library.families.insert(
            face.family.clone(),
            Family {
                name: "Shelf".to_string(),
                faces: vec![Entry {
                    weight: 400,
                    italic: false,
                    asset: asset.clone(),
                }],
            },
        );
        library.faces.insert(asset.clone(), Arc::new(vec![1, 2, 3]));
        let mut fonts = Typesetter::new();

        assert!(!fonts.adopt(&library, std::slice::from_ref(&face)));
        assert!(!fonts.knows(&face));
        assert!(fonts.loaded.contains(&asset), "not worth a second try");
        assert!(!fonts.adopt(&library, std::slice::from_ref(&face)));
    }
}
