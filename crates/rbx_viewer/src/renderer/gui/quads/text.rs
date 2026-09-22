//! Text quads: one rectangle per glyph out of the glyph atlas, each line
//! aligned inside the element's box the way `TextXAlignment`/`TextYAlignment`
//! say, every glyph origin snapped to a whole pixel — Roblox text is crisp,
//! and a bitmap sampled off the pixel grid is not.

use cosmic_text::{CacheKey, Color, Ellipsize, EllipsizeHeightLimit, LayoutRun, PhysicalGlyph};

use super::super::atlas::GLYPHS;
use super::super::pipeline::VertexRaw;
use super::super::text::{unquantised, Typesetter};
use super::shape::{quad, Paint, Shape, Spin, UV_WHOLE};
use super::{extend, Run, Scissor, WHITE};
use crate::scene::{GuiAlign, GuiRect, GuiText, GuiTypeset};

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
/// underlines and strikethroughs. `paint` and `shape` are the element's own
/// fill paint and outline: a `UIGradient` tints the glyphs and a `UICorner`
/// clips them, the same as the background under them.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit(
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
    if text.alpha <= 0.0 && strokes.is_empty() {
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

    for stroke in strokes {
        for [dx, dy] in ring(stroke.thickness) {
            let mut sink = Sink {
                text,
                rect,
                top,
                shift: [dx, dy],
                tint: Some((stroke.color, stroke.alpha)),
                paint,
                shape,
                spin,
                scissor,
                fonts,
                vertices,
                runs,
                line_x: 0.0,
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
        shift: [0.0, 0.0],
        tint: None,
        paint,
        shape,
        spin,
        scissor,
        fonts,
        vertices,
        runs,
        line_x: 0.0,
    };
    for (line, &cut) in lines.iter().zip(&cuts) {
        sink.glyphs(line, cut);
        // After the glyphs, so a strikethrough lies over them.
        sink.line_x = sink.rect.x + offset(text.x_align, rect.width, line.line_w);
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
}

impl Sink<'_> {
    fn glyphs(&mut self, line: &LayoutRun, cut: usize) {
        let text = self.text;
        // Snapped before the glyph offsets go on, so every glyph of the line
        // lands on the same whole-pixel baseline.
        let origin = (
            (self.rect.x + offset(text.x_align, self.rect.width, line.line_w) + self.shift[0])
                .round(),
            (self.top + line.line_y + self.shift[1]).round(),
        );
        let start = self.vertices.len();
        for glyph in line.glyphs.iter().filter(|glyph| glyph.start < cut) {
            let physical = snapped(glyph, origin);
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
fn snapped(glyph: &cosmic_text::LayoutGlyph, origin: (f32, f32)) -> PhysicalGlyph {
    let x = origin.0 + glyph.x + glyph.font_size * glyph.x_offset;
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
/// ponytail: counts `char`s, not grapheme clusters — a combining mark or a
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

/// Where a run of `length` starts inside `extent` for one alignment — the
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
