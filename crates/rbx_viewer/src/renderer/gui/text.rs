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
    Align, Attrs, Buffer, CacheKey, Color, Ellipsize, FontSystem, Metrics, Shaping, Style,
    SwashCache, UnderlineStyle, Weight, Wrap,
};
use rbx_assets::AssetRef;

pub(super) use atlas::{Glyph, GlyphAtlas};

use crate::fonts::{Face, Family, Library};
use crate::scene::{gui_span_face, GuiText, GuiTextMeasure};

/// How many ems tall a line box is: `TextSize` is a line height, not an em
/// (Roblox's docs on `Font`: each font renders "with the line height equal
/// to the `TextSize` property"), and the em is this much smaller.
///
/// Roblox does not say where the ratio comes from, and it is not the face's
/// own ascent plus descent: against Studio captures of the fixtures, Source
/// Sans Pro at `TextSize` 24 draws "Button" 53 px wide with a 13 px cap
/// height, which is a 20 px em — 1.2 — where its hhea metrics (1.257) give
/// 51 px and 12 px and its typographic ones (1.0) a fifth too much. Gotham
/// SSm (hhea 1.2) lands on Studio's 14 px x-height at 29 and its `TextScaled`
/// sizes with the same 1.2; Fredoka One's hhea 1.21 is too close to tell
/// apart at the sizes the fixtures draw it. One constant fits all three.
const LINE_EM: f32 = 1.2;

pub(super) struct Typesetter {
    system: FontSystem,
    cache: SwashCache,
    pub(super) atlas: GlyphAtlas,
    /// Family JSON reference → the faces of it that have landed, under the
    /// family name fontdb filed them by, which is the name inside the face
    /// file, not the JSON's `name`.
    families: HashMap<AssetRef, Family>,
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
            let Some(entry) = library.entry_of(face) else {
                continue;
            };
            let Some(bytes) = library.faces.get(&entry.asset) else {
                continue;
            };
            if !self.loaded.insert(entry.asset.clone()) {
                continue;
            }
            let db = self.system.db_mut();
            let known: HashSet<_> = db.faces().map(|info| info.id).collect();
            db.load_font_data(bytes.to_vec());
            let fresh: Vec<_> = db
                .faces()
                .filter(|info| !known.contains(&info.id))
                .cloned()
                .collect();
            for mut info in fresh {
                // The family JSON, not the face's own OS/2 table, says what a
                // face weighs: Roblox's `GothamSSm-Bold.otf` calls itself
                // regular and its Book face lighter still, so matched on the
                // tables a 400 request lands on Bold. Re-filed under the
                // JSON's weight and style, `FontFace.weight` picks the face
                // Roblox picks. The family name does come from the tables
                // ("Gotham SSm" for `GothamSSm.json`), being what fontdb
                // matches on.
                db.remove_face(info.id);
                info.weight = Weight(entry.weight);
                info.style = match entry.italic {
                    true => Style::Italic,
                    false => Style::Normal,
                };
                if let Some((name, _)) = info.families.first() {
                    let family =
                        self.families
                            .entry(face.family.clone())
                            .or_insert_with(|| Family {
                                name: name.clone(),
                                faces: Vec::new(),
                            });
                    if !family.faces.contains(entry) {
                        family.faces.push(entry.clone());
                    }
                    added = true;
                }
                db.push_face_info(info);
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
    ///
    /// `size` is a line box, `LINE_EM` ems tall, and `LineHeight` scales the
    /// spacing between lines on top of that (the docs' "multiple of the
    /// font's em square" read as a multiple of `TextSize`, since 1.0 has to
    /// give lines `TextSize` tall). cosmic-text centres a face's ascent plus
    /// descent inside the line, which is where Studio's baseline lands.
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
        let mut buffer = Buffer::new(system, metrics(size, text.line_height));
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
                    attrs = attrs.metrics(metrics(size * scale, text.line_height));
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

/// The em and line spacing of a `size`-pixel line box.
fn metrics(size: f32, line_height: f32) -> Metrics {
    Metrics::new(size / LINE_EM, size * line_height)
}

/// The attributes one face shapes with: the family by the name fontdb knows
/// it under, at the weight and style of the face that landed nearest the
/// request — cosmic-text serves a weight only from a face of exactly that
/// weight, so asked for the 700 a family lacks it would pass over the
/// family's lone 400 face for a system font. The sans-serif fallback, at the
/// requested weight, while the family is unknown.
fn attrs<'a>(families: &'a HashMap<AssetRef, Family>, face: &Face) -> Attrs<'a> {
    let landed = families
        .get(&face.family)
        .and_then(|family| Some((family, family.closest(face.weight, face.italic)?)));
    let (family, weight, italic) = match landed {
        Some((family, entry)) => (
            cosmic_text::Family::Name(&family.name),
            entry.weight,
            entry.italic,
        ),
        None => (cosmic_text::Family::SansSerif, face.weight, face.italic),
    };
    Attrs::new()
        .family(family)
        .weight(Weight(weight))
        .style(match italic {
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
    use crate::fonts::Entry;
    use crate::scene::{GuiAlign, GuiTextSpan};

    fn plain(face: Face, size: f32) -> GuiText {
        GuiText {
            spans: vec![GuiTextSpan::plain("Ag")],
            color: [1.0; 3],
            alpha: 1.0,
            size,
            scaled: false,
            wrapped: false,
            x_align: GuiAlign::Start,
            y_align: GuiAlign::Start,
            face,
            line_height: 1.0,
            stroke: None,
            truncate: false,
            max_graphemes: None,
            automatic: [false, false],
            size_bounds: None,
        }
    }

    /// A typesetter that believes `family` has landed with one upright face
    /// of `weight` — with no font file at all, which the attributes and
    /// metrics a shape asks for never need.
    fn believing(family: &AssetRef, weight: u16) -> Typesetter {
        let mut fonts = Typesetter::new();
        fonts.families.insert(
            family.clone(),
            Family {
                name: "Believed".to_string(),
                faces: vec![Entry {
                    weight,
                    italic: false,
                    asset: AssetRef::Native("fonts/Believed.ttf".to_string()),
                }],
            },
        );
        fonts
    }

    // Roblox's docs make `TextSize` the line height; the em is a fifth
    // smaller, for a `<font size>` as much as for the base size, and
    // `LineHeight` spaces the lines without touching the em.
    #[test]
    fn text_size_is_the_line_box_and_the_em_a_fifth_smaller() {
        let face = Face::named("Believed", 400, false);
        let mut fonts = believing(&face.family, 400);
        let mut text = plain(face, 24.0);
        text.spans.push(GuiTextSpan {
            size: Some(12.0),
            ..GuiTextSpan::plain("small")
        });
        text.line_height = 1.5;

        let buffer = fonts.shape(&text, 24.0, None, None);
        assert_eq!(buffer.metrics(), Metrics::new(20.0, 36.0));
        let spans = buffer.lines[0].attrs_list();
        assert_eq!(spans.get_span(0).metrics_opt, None, "the buffer's own");
        assert_eq!(
            spans.get_span(2).metrics_opt,
            Some(Metrics::new(10.0, 18.0).into()),
            "a <font size> is a line box too"
        );
    }

    // A family with a single regular face serves a bold request with that
    // face, as Roblox does, rather than with some bold system font.
    #[test]
    fn a_weight_the_family_lacks_shapes_in_the_face_that_landed() {
        let face = Face::named("Believed", 700, false);
        let mut fonts = believing(&face.family, 400);

        let buffer = fonts.shape(&plain(face, 24.0), 24.0, None, None);
        let attrs = buffer.lines[0].attrs_list().get_span(0);
        assert_eq!(attrs.family, cosmic_text::Family::Name("Believed"));
        assert_eq!(attrs.weight, Weight(400));

        // Only a family that never landed keeps the request as asked, for
        // the fallback to make of what it can.
        let unknown = Face::named("Unknown", 700, false);
        let buffer = fonts.shape(&plain(unknown, 24.0), 24.0, None, None);
        let attrs = buffer.lines[0].attrs_list().get_span(0);
        assert_eq!(attrs.family, cosmic_text::Family::SansSerif);
        assert_eq!(attrs.weight, Weight(700));
    }

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
