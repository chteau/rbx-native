//! Text quads: one rectangle per glyph out of the glyph atlas, each line
//! aligned inside the element's box the way `TextXAlignment`/`TextYAlignment`
//! say, every glyph origin snapped to a whole pixel, Roblox text is crisp,
//! and a bitmap sampled off the pixel grid is not.

use std::ops::Range;

use cosmic_text::{
    CacheKey, Color, Ellipsize, EllipsizeHeightLimit, LayoutRun, LineIter, PhysicalGlyph,
};
use rbx_dom::Ref;

use super::super::atlas::GLYPHS;
use super::super::pipeline::VertexRaw;
use super::super::text::{
    scroll_to, unquantised, GuiTextEdit, GuiTextLine, GuiTextMap, Typesetter, SELECTED_TEXT,
    SELECTION,
};
use super::shape::{quad, Paint, Shape, Spin, FILL, UV_WHOLE};
use super::{extend, Run, Scissor, WHITE};
use crate::scene::{srgb_to_linear, GuiAlign, GuiRect, GuiText, GuiTypeset};

/// One outline drawn under the fill: the run again in `color` at `alpha`,
/// `thickness` pixels out in each of the eight directions.
///
/// Roblox publishes no algorithm for `TextStroke`; its own docs only say a
/// stroke is "multiple renderings of the same transparency", so this is the
/// eight-neighbour rendering every bitmap outline starts from. A `UIStroke`
/// in `ApplyStrokeMode.Contextual` on a text object is one more of these
/// with its own `Thickness`, appended to what [`strokes`] returns.
pub(in crate::renderer::gui) struct Stroke {
    pub(in crate::renderer::gui) color: [f32; 3],
    pub(in crate::renderer::gui) alpha: f32,
    pub(in crate::renderer::gui) thickness: f32,
}

/// The strokes the text's own properties ask for: `TextStrokeColor3` at one
/// pixel, where `TextStrokeTransparency` shows it at all.
pub(super) fn strokes(text: &GuiText) -> Vec<Stroke> {
    text.stroke
        .map(|(color, alpha)| Stroke {
            color,
            alpha,
            thickness: 1.0,
        })
        .into_iter()
        .collect()
}

/// Every quad of one element's text, strokes first, then the fill with its
/// underlines and strikethroughs. `paint` is the element's own fill paint,
/// so a `UIGradient` tints the glyphs like the background under them, and
/// `shape` its text's ([`Shape::text_of`]), which no `UICorner` cuts. The
/// `TextBox` being typed in (`Typesetter::edit`) gets its selection under
/// all that and leaves where its characters went in `Typesetter::edit_map`.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit(
    referent: Ref,
    rect: &GuiRect,
    typeset: &GuiTypeset,
    strokes: &[Stroke],
    (paint, shape, spin): (&Paint, &Shape, &Spin),
    scissor: Option<Scissor>,
    fonts: &mut Typesetter,
    vertices: &mut Vec<VertexRaw>,
    runs: &mut Vec<Run>,
) {
    let text = &typeset.text;
    let edit = fonts.edit.clone().filter(|edit| edit.referent == referent);
    if text.alpha <= 0.0 && strokes.is_empty() && edit.is_none() {
        return;
    }
    let wrapped = text.wrapped || text.scaled;
    // An unwrapped line still needs the width to know where an ellipsis goes.
    let width = (wrapped || text.truncate).then_some(rect.width);
    let ellipsize = text.truncate.then_some(Ellipsize::End(match wrapped {
        true => EllipsizeHeightLimit::Height(rect.height),
        false => EllipsizeHeightLimit::Lines(1),
    }));
    let buffer = fonts.shape_cached(text, typeset.size, width, ellipsize);
    let lines: Vec<LayoutRun> = buffer.layout_runs().collect();
    if lines.is_empty() {
        if let Some(edit) = &edit {
            let height = buffer.metrics().line_height;
            let top = rect.y + offset(text.y_align, rect.height, height);
            fonts.edit_map = Some(empty_map(edit, rect, text, top, height));
        }
        return;
    }

    // Roblox's docs on `TextWrapped`: a line that would push the text past the
    // box's height "will not be rendered at all". The first line always is.
    let kept = match wrapped {
        true => {
            let mut height = 0.0;
            lines
                .iter()
                .take_while(|line| {
                    height += line.line_height;
                    height <= rect.height + 0.5
                })
                .count()
                .max(1)
        }
        false => lines.len(),
    };
    let lines = &lines[..kept];
    let total: f32 = lines.iter().map(|line| line.line_height).sum();
    let top = rect.y + offset(text.y_align, rect.height, total);
    let cuts = visible_cuts(text, lines);
    let starts = paragraph_starts(text);
    let mut scroll = 0.0;
    let mut selected = None;
    if let Some(edit) = &edit {
        let mut map = match edit.empty {
            true => empty_map(edit, rect, text, top, lines[0].line_height),
            false => GuiTextMap {
                lines: lines
                    .iter()
                    .map(|line| map_line(fonts, rect, text, top, line, &starts))
                    .collect(),
                scroll: 0.0,
            },
        };
        if !wrapped {
            if let Some([x, _, _]) = map.caret(edit.caret) {
                scroll = scroll_to(edit.scroll, x, [rect.x, rect.x + rect.width]);
            }
        }
        for line in &mut map.lines {
            for stop in &mut line.stops {
                stop.1 -= scroll;
            }
        }
        map.scroll = scroll;
        if !edit.empty && !edit.selection.is_empty() {
            selected = Some(edit.selection.clone());
            let start = vertices.len();
            for line in &map.lines {
                if let Some(band) = band(line, &edit.selection) {
                    let paint = Paint {
                        color: SELECTION.map(srgb_to_linear),
                        alpha: 1.0,
                        band: FILL,
                        gradient: None,
                    };
                    quad(&band, UV_WHOLE, &paint, shape, spin, vertices);
                }
            }
            extend(runs, WHITE, scissor, start..vertices.len());
        }
        fonts.edit_map = Some(map);
    }

    for stroke in strokes {
        for [dx, dy] in ring(stroke.thickness) {
            let mut sink = Sink {
                text,
                rect,
                top,
                shift: [dx - scroll, dy],
                tint: Some((stroke.color, stroke.alpha)),
                paint,
                shape,
                spin,
                scissor,
                fonts,
                vertices,
                runs,
                line_x: 0.0,
                selected: None,
                base: 0,
            };
            for (line, &cut) in lines.iter().zip(&cuts) {
                sink.glyphs(line, cut);
            }
        }
    }
    let mut sink = Sink {
        text,
        rect,
        top,
        shift: [-scroll, 0.0],
        tint: None,
        paint,
        shape,
        spin,
        scissor,
        fonts,
        vertices,
        runs,
        line_x: 0.0,
        selected,
        base: 0,
    };
    for (line, &cut) in lines.iter().zip(&cuts) {
        sink.base = starts.get(line.line_i).copied().unwrap_or(0);
        sink.glyphs(line, cut);
        // After the glyphs, so a strikethrough lies over them.
        sink.line_x =
            sink.rect.x + offset(text.x_align, rect.width, sink.fonts.line_width(line)) - scroll;
        cosmic_text::render_decoration(&mut sink, line, Color::rgb(0, 0, 0));
    }
}

/// One pass over the lines: the fill, or one offset of one stroke.
struct Sink<'a> {
    text: &'a GuiText,
    rect: &'a GuiRect,
    /// Where the first line's top lands, `TextYAlignment` applied.
    top: f32,
    /// The stroke offset, zero for the fill.
    shift: [f32; 2],
    /// A stroke's colour and alpha in place of every glyph's own.
    tint: Option<([f32; 3], f32)>,
    /// The element's fill paint (for its gradient) and outline (for its
    /// rounding); the colour and alpha are the glyph's own.
    paint: &'a Paint<'a>,
    shape: &'a Shape,
    spin: &'a Spin,
    scissor: Option<Scissor>,
    fonts: &'a mut Typesetter,
    vertices: &'a mut Vec<VertexRaw>,
    runs: &'a mut Vec<Run>,
    /// The current line's left edge, for the decorations drawn through
    /// [`cosmic_text::Renderer`], which only know their offset along it.
    line_x: f32,
    /// The edited box's selected bytes, drawn in the selection's text colour.
    selected: Option<Range<usize>>,
    /// The current line's paragraph's byte offset in the whole text.
    base: usize,
}

impl Sink<'_> {
    fn glyphs(&mut self, line: &LayoutRun, cut: usize) {
        let text = self.text;
        let (xs, width) = self.fonts.glyph_xs(line);
        // Snapped before the glyph offsets go on, so every glyph of the line
        // lands on the same whole-pixel baseline.
        let origin = (
            (self.rect.x + offset(text.x_align, self.rect.width, width) + self.shift[0]).round(),
            (self.top + line.line_y + self.shift[1]).round(),
        );
        let start = self.vertices.len();
        for (glyph, &x) in line
            .glyphs
            .iter()
            .zip(&xs)
            .filter(|(glyph, _)| glyph.start < cut)
        {
            let physical = snapped(glyph, x, origin);
            let Some(slot) = self.fonts.glyph(physical.cache_key) else {
                continue;
            };
            let (color, alpha) = match (self.tint, slot.color) {
                // A stroke is an outline of the glyph's coverage, which a
                // colour bitmap has none of to speak of.
                (Some(_), true) => continue,
                (Some(tint), false) => tint,
                // Emoji keep their own colours: Roblox's docs on `Text` say
                // `TextColor3` does not affect them.
                (None, true) => ([1.0; 3], text.alpha),
                (None, false)
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|range| range.contains(&(self.base + glyph.start))) =>
                {
                    (SELECTED_TEXT, text.alpha)
                }
                (None, false) => {
                    text.spans
                        .get(glyph.metadata)
                        .map_or((text.color, text.alpha), |span| {
                            (
                                span.color.unwrap_or(text.color),
                                span.alpha.unwrap_or(text.alpha),
                            )
                        })
                }
            };
            if alpha <= 0.0 {
                continue;
            }
            let side = self.fonts.atlas.side() as f32;
            let box_ = GuiRect {
                x: (physical.x + slot.left) as f32,
                y: (physical.y - slot.top) as f32,
                width: slot.width as f32,
                height: slot.height as f32,
            };
            let uv = [
                slot.x as f32 / side,
                slot.y as f32 / side,
                (slot.x + slot.width) as f32 / side,
                (slot.y + slot.height) as f32 / side,
            ];
            let paint = Paint {
                color,
                alpha,
                ..*self.paint
            };
            quad(
                &box_,
                ([uv[0], uv[1]], [uv[2], uv[3]]),
                &paint,
                self.shape,
                self.spin,
                self.vertices,
            );
        }
        extend(self.runs, GLYPHS, self.scissor, start..self.vertices.len());
    }
}

impl cosmic_text::Renderer for Sink<'_> {
    fn rectangle(&mut self, x: i32, y: i32, width: u32, height: u32, color: Color) {
        let (color, alpha) = unquantised(color);
        if alpha <= 0.0 {
            return;
        }
        let band = GuiRect {
            x: self.line_x.round() + x as f32,
            y: self.top.round() + y as f32,
            width: width as f32,
            height: height as f32,
        };
        let start = self.vertices.len();
        let paint = Paint {
            color,
            alpha,
            ..*self.paint
        };
        quad(
            &band,
            UV_WHOLE,
            &paint,
            self.shape,
            self.spin,
            self.vertices,
        );
        extend(self.runs, WHITE, self.scissor, start..self.vertices.len());
    }

    /// Never called: glyphs go through [`Sink::glyphs`], which knows the
    /// span each one came from.
    fn glyph(&mut self, _glyph: PhysicalGlyph, _color: Color) {}
}

/// [`cosmic_text::LayoutGlyph::physical`] with the position rounded first:
/// a whole-pixel position bins to no subpixel offset at all, so the same
/// bitmap serves the glyph wherever it lands and always sits on the grid.
fn snapped(glyph: &cosmic_text::LayoutGlyph, at: f32, origin: (f32, f32)) -> PhysicalGlyph {
    let x = origin.0 + at + glyph.font_size * glyph.x_offset;
    let y = origin.1 + glyph.y - glyph.font_size * glyph.y_offset;
    let (cache_key, x, y) = CacheKey::new(
        glyph.font_id,
        glyph.glyph_id,
        glyph.font_size,
        (x.round(), y.round()),
        glyph.font_weight,
        glyph.cache_key_flags,
    );
    PhysicalGlyph { cache_key, x, y }
}

/// Per line, the byte offset past which `MaxVisibleGraphemes` hides the
/// glyphs; `usize::MAX` where nothing is hidden. The layout is untouched, as
/// Roblox's docs say: "the layout will be calculated as if all graphemes
/// are visible".
///
/// ponytail: counts `char`s, not grapheme clusters, a combining mark or a
/// ZWJ sequence counts as several; swap in `unicode-segmentation` (already in
/// the lock) if a typewriter effect over such text ever matters.
fn visible_cuts(text: &GuiText, lines: &[LayoutRun]) -> Vec<usize> {
    let Some(limit) = text.max_graphemes else {
        return vec![usize::MAX; lines.len()];
    };
    let mut remaining = limit;
    let mut source_line = None;
    let mut cut = usize::MAX;
    lines
        .iter()
        .map(|line| {
            // Wrapped runs of one source line share its text and its cut.
            if source_line != Some(line.line_i) {
                source_line = Some(line.line_i);
                cut = line
                    .text
                    .char_indices()
                    .nth(remaining)
                    .map_or(line.text.len(), |(at, _)| at);
                // The line break itself is a grapheme.
                remaining = remaining.saturating_sub(line.text.chars().count() + 1);
            }
            cut
        })
        .collect()
}

/// Where each paragraph (each line the text's own line breaks start)
/// begins in the whole text, which is what a run's byte offsets count from.
fn paragraph_starts(text: &GuiText) -> Vec<usize> {
    let whole: String = text.spans.iter().map(|span| span.text.as_str()).collect();
    LineIter::new(&whole)
        .map(|(range, _)| range.start)
        .collect()
}

/// One drawn line's caret stops, where [`Sink::glyphs`] puts its glyphs.
fn map_line(
    fonts: &mut Typesetter,
    rect: &GuiRect,
    text: &GuiText,
    top: f32,
    line: &LayoutRun,
    starts: &[usize],
) -> GuiTextLine {
    let base = starts.get(line.line_i).copied().unwrap_or(0);
    let (xs, width) = fonts.glyph_xs(line);
    let left = (rect.x + offset(text.x_align, rect.width, width)).round();
    let mut stops: Vec<(usize, f32)> = line
        .glyphs
        .iter()
        .zip(&xs)
        .map(|(glyph, &x)| (base + glyph.start, left + x))
        .collect();
    let end = line.glyphs.iter().map(|glyph| glyph.end).max();
    stops.push((base + end.unwrap_or(0), left + width));
    stops.sort_by_key(|&(at, _)| at);
    stops.dedup_by_key(|&mut (at, _)| at);
    GuiTextLine {
        top: (top + line.line_top).round(),
        height: line.line_height,
        stops,
    }
}

/// The one caret stop of a box showing its placeholder: where an empty line
/// would start.
fn empty_map(
    edit: &GuiTextEdit,
    rect: &GuiRect,
    text: &GuiText,
    top: f32,
    height: f32,
) -> GuiTextMap {
    GuiTextMap {
        lines: vec![GuiTextLine {
            top: top.round(),
            height,
            stops: vec![(0, (rect.x + offset(text.x_align, rect.width, 0.0)).round())],
        }],
        scroll: edit.scroll.min(0.0),
    }
}

/// The selection behind `line`: between the stops it covers on the line,
/// the line box tall. A line break selected adds nothing, as in Studio.
fn band(line: &GuiTextLine, selection: &Range<usize>) -> Option<GuiRect> {
    let (&(first, _), &(last, _)) = (line.stops.first()?, line.stops.last()?);
    let (from, to) = (selection.start.max(first), selection.end.min(last));
    let x_at = |offset: usize| {
        line.stops
            .iter()
            .rev()
            .find(|&&(at, _)| at <= offset)
            .map(|&(_, x)| x)
    };
    let (left, right) = (x_at(from)?, x_at(to)?);
    (from < to && right > left).then_some(GuiRect {
        x: left,
        y: line.top,
        width: right - left,
        height: line.height,
    })
}

/// The eight offsets a stroke of `thickness` pixels is drawn at.
fn ring(thickness: f32) -> [[f32; 2]; 8] {
    let t = thickness.max(1.0);
    [
        [-t, -t],
        [0.0, -t],
        [t, -t],
        [-t, 0.0],
        [t, 0.0],
        [-t, t],
        [0.0, t],
        [t, t],
    ]
}

/// Where a run of `length` starts inside `extent` for one alignment, the
/// same rule the layout applies to a `UIListLayout`.
fn offset(align: GuiAlign, extent: f32, length: f32) -> f32 {
    match align {
        GuiAlign::Start => 0.0,
        GuiAlign::Center => (extent - length) * 0.5,
        GuiAlign::End => extent - length,
    }
}

#[cfg(test)]
mod tests;
