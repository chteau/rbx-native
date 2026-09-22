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
    Align, Angle, Attrs, Buffer, CacheKey, CacheKeyFlags, Color, Ellipsize, FontSystem, Metrics,
    Shaping, Style, SwashCache, SwashImage, Transform, UnderlineStyle, Weight, Wrap,
};
use rbx_assets::AssetRef;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Format, Vector};

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

/// `Enum.FontWeight.SemiBold`: from here up a face is bold — Roblox's docs on
/// `Font.Bold` say it "becomes `true` if the weight is `SemiBold` or thicker"
/// — so a request at or past it on a family whose nearest face falls short
/// is served by dilating that face's outlines. The docs say nothing about
/// how Studio serves such a weight; that it synthesises one is measured
/// (Fredoka One, a single-face family, at 600: strokes half again as thick).
const SEMI_BOLD: u16 = 600;

/// How far each side of an outline moves out for a synthesised bold, in
/// ems: FreeType's classic `FT_GlyphSlot_Embolden` strength, which against a
/// Studio capture of Fredoka One at 600 lands the chevrons' stroke within a
/// pixel.
const FAKE_BOLD_EM: f32 = 1.0 / 24.0;

/// Asks [`Typesetter::glyph`] for the dilated outline. cosmic-text has no
/// such flag of its own (it synthesises italic, not bold); this is a bit it
/// leaves unused, carried through shaping untouched to the glyph's cache
/// key, which is what tells one glyph's bitmap from another.
const FAKE_BOLD: CacheKeyFlags = CacheKeyFlags::from_bits_retain(1 << 8);

pub(super) struct Typesetter {
    system: FontSystem,
    cache: SwashCache,
    /// For the glyphs [`SwashCache`] cannot draw: the synthesised bold ones.
    scaler: ScaleContext,
    pub(super) atlas: GlyphAtlas,
    /// Family JSON reference → the faces of it that have landed, under the
    /// family name fontdb filed them by, which is the name inside the face
    /// file, not the JSON's `name`.
    families: HashMap<AssetRef, Family>,
    /// Every face file already handed to the font system.
    loaded: HashSet<AssetRef>,
    /// Bounds already measured, by what decides them (see [`measure_key`]).
    /// A `TextScaled` label measures its whole string once per step of the
    /// size search on every layout, and a layout follows every edit, so a
    /// canvas drag through a text-heavy screen was spent re-shaping labels
    /// nothing had changed. Dropped whenever a face lands, which is the one
    /// thing that changes a measurement without changing the text.
    measured: HashMap<String, [f32; 2]>,
    /// Buffers already shaped for drawing, by the whole text (colours
    /// included: a decoration takes its colour from the shaping) and what
    /// it was shaped to. Dropped with [`Typesetter::measured`].
    shaped: HashMap<String, Buffer>,
}

/// More measurements than any one screen holds; past it the cache starts
/// over rather than growing without end across edits that change the text.
const MEASURED_CAP: usize = 16_384;

/// Everything a measurement depends on — the runs and the face, the size
/// the spans scale from, the spacing, the wrapping, and the size and width
/// asked for — and nothing it does not (colour, alignment, the stroke).
fn measure_key(text: &GuiText, size: f32, max_width: Option<f32>) -> String {
    let spans: Vec<_> = text
        .spans
        .iter()
        .map(|span| {
            (
                &span.text,
                span.bold,
                span.italic,
                span.size,
                &span.family,
                span.weight,
            )
        })
        .collect();
    format!(
        "{spans:?}|{:?}|{}|{}|{}|{}|{size}|{max_width:?}",
        text.face, text.size, text.line_height, text.wrapped, text.scaled
    )
}

impl Typesetter {
    /// Starts from the system's fonts: text in a family that has not landed
    /// yet — or never will, a face behind a cloud id with no API key — shapes
    /// in the locale's sans-serif rather than vanishing.
    pub(super) fn new() -> Self {
        Typesetter {
            system: FontSystem::new(),
            cache: SwashCache::new(),
            scaler: ScaleContext::new(),
            atlas: GlyphAtlas::new(),
            families: HashMap::new(),
            loaded: HashSet::new(),
            measured: HashMap::new(),
            shaped: HashMap::new(),
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
        if added {
            self.measured.clear();
            self.shaped.clear();
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

    /// [`Typesetter::shape`], reusing the buffer an identical call shaped
    /// before — a layout follows every edit, and most text on a screen is
    /// not what the edit touched.
    pub(super) fn shape_cached(
        &mut self,
        text: &GuiText,
        size: f32,
        width: Option<f32>,
        ellipsize: Option<Ellipsize>,
    ) -> Buffer {
        let key = format!("{text:?}|{size}|{width:?}|{ellipsize:?}");
        if let Some(buffer) = self.shaped.get(&key) {
            return buffer.clone();
        }
        let buffer = self.shape(text, size, width, ellipsize);
        if self.shaped.len() >= MEASURED_CAP {
            self.shaped.clear();
        }
        self.shaped.insert(key, buffer.clone());
        buffer
    }

    /// The glyph's place in the atlas, rasterising it on first sight; `None`
    /// for one with no bitmap.
    pub(super) fn glyph(&mut self, key: CacheKey) -> Option<Glyph> {
        if let Some(known) = self.atlas.get(&key) {
            return known;
        }
        if key.flags.contains(FAKE_BOLD) {
            let image = self.embolden(key);
            return self.atlas.insert(key, image.as_ref());
        }
        let image = self.cache.get_image(&mut self.system, key).as_ref();
        self.atlas.insert(key, image)
    }

    /// The glyph as `SwashCache` would draw it — same sources, hinting and
    /// synthesised slant — with every outline pushed out `FAKE_BOLD_EM` of
    /// the em first. Roblox's own faces are static, so the variable-weight
    /// axis cosmic-text would also set is left alone.
    ///
    /// ponytail: the advances stay the face's own, so a run of synthesised
    /// bold sits `FAKE_BOLD_EM` tighter than Studio's; add the strength to
    /// each flagged glyph's advance in `shape` if that ever shows.
    fn embolden(&mut self, key: CacheKey) -> Option<SwashImage> {
        let font = self.system.get_font(key.font_id, key.font_weight)?;
        let size = f32::from_bits(key.font_size_bits);
        let mut scaler = self
            .scaler
            .builder(font.as_swash())
            .size(size)
            .hint(true)
            .build();
        Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .offset(Vector::new(key.x_bin.as_float(), key.y_bin.as_float()))
        .transform(
            key.flags
                .contains(CacheKeyFlags::FAKE_ITALIC)
                .then(|| Transform::skew(Angle::from_degrees(14.0), Angle::from_degrees(0.0))),
        )
        .embolden(size * FAKE_BOLD_EM)
        .render(&mut scaler, key.glyph_id)
    }
}

impl GuiTextMeasure for Typesetter {
    fn measure(&mut self, text: &GuiText, size: f32, max_width: Option<f32>) -> [f32; 2] {
        let key = measure_key(text, size, max_width);
        if let Some(&known) = self.measured.get(&key) {
            return known;
        }
        let measured = bounds(&self.shape(text, size, max_width, None));
        if self.measured.len() >= MEASURED_CAP {
            self.measured.clear();
        }
        self.measured.insert(key, measured);
        measured
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
/// it under, at the weight of the face that landed nearest the request —
/// cosmic-text serves a weight only from a face of exactly that weight, so
/// asked for the 700 a family lacks it would pass over the family's lone 400
/// face for a system font. What that face cannot give is synthesised, as
/// Studio does: the style stays the requested one (cosmic-text slants a face
/// that has no italic of its own), and a bold weight on a face short of
/// [`SEMI_BOLD`] asks for [`FAKE_BOLD`]. The sans-serif fallback, at the
/// requested weight, while the family is unknown.
fn attrs<'a>(families: &'a HashMap<AssetRef, Family>, face: &Face) -> Attrs<'a> {
    let landed = families
        .get(&face.family)
        .and_then(|family| Some((family, family.closest(face.weight, face.italic)?)));
    let (family, weight, flags) = match landed {
        Some((family, entry)) => (
            cosmic_text::Family::Name(&family.name),
            entry.weight,
            match face.weight >= SEMI_BOLD && entry.weight < SEMI_BOLD {
                true => FAKE_BOLD,
                false => CacheKeyFlags::empty(),
            },
        ),
        None => (
            cosmic_text::Family::SansSerif,
            face.weight,
            CacheKeyFlags::empty(),
        ),
    };
    Attrs::new()
        .family(family)
        .weight(Weight(weight))
        .style(match face.italic {
            true => Style::Italic,
            false => Style::Normal,
        })
        .cache_key_flags(flags)
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
mod tests;
