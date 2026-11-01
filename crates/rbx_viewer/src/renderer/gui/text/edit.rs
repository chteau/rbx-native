//! A `TextBox` being typed in, drawn by the canvas itself so its text,
//! wrapping and glyph positions are the ones the box shows unfocused: the
//! selection behind the text, the selected glyphs in Studio's selection
//! colours, and a single line scrolled to keep the caret in the box. What
//! the draw says about where each character lies goes back to the host,
//! which places the caret and hit-tests clicks on it.

use std::ops::Range;

use rbx_dom::Ref;

/// Studio's selection behind selected `TextBox` text, measured off a
/// capture: #6BA1F9 whatever the box's colours. sRGB, like a `Color3` out
/// of a place, so it goes through `srgb_to_linear` before it's drawn.
pub(in crate::renderer::gui) const SELECTION: [f32; 3] =
    [107.0 / 255.0, 161.0 / 255.0, 249.0 / 255.0];
/// The selected glyphs, white over the selection on a light box with dark
/// text as much as on a dark one.
pub(in crate::renderer::gui) const SELECTED_TEXT: [f32; 3] = [1.0; 3];

/// The `TextBox` being typed in, as the host edits it.
#[derive(Debug, Clone, PartialEq)]
pub struct GuiTextEdit {
    pub referent: Ref,
    /// Byte offsets into `Text`, `start <= end`.
    pub selection: Range<usize>,
    /// The caret's byte offset, one end of `selection`.
    pub caret: usize,
    /// How far an unwrapped line is scrolled left, as the last draw left it
    /// (see [`GuiTextMap::scroll`]).
    pub scroll: f32,
    /// `Text` is empty and the box shows `PlaceholderText`, whose glyphs
    /// are none of the text's.
    pub empty: bool,
}

/// Where the edited box's characters lie, in canvas pixels.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GuiTextMap {
    /// The drawn lines, top to bottom; a wrapped box's lines past its height
    /// are not drawn (Roblox's docs on `TextWrapped`) and are not here.
    pub lines: Vec<GuiTextLine>,
    /// The scroll the draw used: the edit's, moved just enough to keep the
    /// caret inside the box, as Studio does with an unwrapped line.
    pub scroll: f32,
}

/// One drawn line.
#[derive(Debug, Clone, PartialEq)]
pub struct GuiTextLine {
    pub top: f32,
    pub height: f32,
    /// Every caret stop on the line, by byte offset ascending: before each
    /// glyph and after the last.
    pub stops: Vec<(usize, f32)>,
}

impl GuiTextMap {
    /// The line `offset` sits on: the last one starting at or before it, so
    /// the offset where a wrapped line breaks lies at the start of the next.
    pub fn line_of(&self, offset: usize) -> Option<usize> {
        let index = self
            .lines
            .iter()
            .rposition(|line| line.stops.first().is_some_and(|&(at, _)| at <= offset))
            .unwrap_or(0);
        (!self.lines.is_empty()).then_some(index)
    }

    /// Where the caret at `offset` stands: `x, top, height`.
    pub fn caret(&self, offset: usize) -> Option<[f32; 3]> {
        let line = &self.lines[self.line_of(offset)?];
        let x = line
            .stops
            .iter()
            .rev()
            .find(|&&(at, _)| at <= offset)
            .or(line.stops.first())
            .map(|&(_, x)| x)?;
        Some([x, line.top, line.height])
    }

    /// The stop nearest `x` on line `index`, where Up and Down land.
    pub fn nearest(&self, index: usize, x: f32) -> Option<usize> {
        self.lines
            .get(index)?
            .stops
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map(|&(at, _)| at)
    }

    /// The stop a click at a point lands on: the line under `y` (the first
    /// or last past either end), then on it the stop before the glyph under
    /// `x`, or the one after it once `x` is two thirds of the way across.
    /// Measured in Studio over P, l, o, k, w, m and a space in BuilderSans
    /// 15: the caret goes past a glyph only when `3 * (x - start) > 2 *
    /// advance`, never at the midpoint. Studio's mouse is on whole pixels,
    /// so the point is too: a widget's origin can sit a hair off one.
    pub fn hit(&self, [x, y]: [f32; 2]) -> Option<usize> {
        let (x, y) = (x.round(), y.round());
        let index = self
            .lines
            .iter()
            .position(|line| y < line.top + line.height)
            .unwrap_or(self.lines.len().checked_sub(1)?);
        let stops = &self.lines[index].stops;
        let after = stops.iter().rposition(|&(_, at)| at <= x).unwrap_or(0);
        let pick = match (stops.get(after), stops.get(after + 1)) {
            (Some(&(_, start)), Some(&(next, end))) if 3.0 * (x - start) > 2.0 * (end - start) => {
                next
            }
            (Some(&(at, _)), _) => at,
            (None, _) => return None,
        };
        Some(pick)
    }
}

/// The scroll that keeps a caret at `caret_x` (unscrolled) inside
/// `left..right`, moving `scroll` as little as it takes. Studio parks the
/// caret on the box's last pixel column, not past it.
pub(in crate::renderer::gui) fn scroll_to(
    scroll: f32,
    caret_x: f32,
    [left, right]: [f32; 2],
) -> f32 {
    let last = (right - 1.0).max(left);
    if caret_x - scroll > last {
        caret_x - last
    } else if caret_x - scroll < left {
        (caret_x - left).max(0.0)
    } else {
        scroll
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(top: f32, stops: &[(usize, f32)]) -> GuiTextLine {
        GuiTextLine {
            top,
            height: 16.0,
            stops: stops.to_vec(),
        }
    }

    #[test]
    fn a_wrap_point_belongs_to_the_next_line() {
        let map = GuiTextMap {
            lines: vec![
                line(0.0, &[(0, 0.0), (1, 8.0), (2, 12.0)]),
                line(16.0, &[(2, 0.0), (3, 9.0)]),
            ],
            scroll: 0.0,
        };
        assert_eq!(map.caret(1), Some([8.0, 0.0, 16.0]));
        assert_eq!(map.caret(2), Some([0.0, 16.0, 16.0]));
        assert_eq!(map.hit([7.0, 40.0]), Some(3), "below the last line");
    }

    // Studio's numbers off the chat fixture's note: "P" from 16 to 24, "l"
    // to 27, then "k" from 127 to 133 and "w" from 136 to 145.
    #[test]
    fn a_click_passes_a_glyph_two_thirds_of_the_way_across() {
        let map = GuiTextMap {
            lines: vec![line(
                0.0,
                &[
                    (0, 16.0),
                    (1, 24.0),
                    (2, 27.0),
                    (3, 127.0),
                    (4, 133.0),
                    (5, 136.0),
                    (6, 145.0),
                ],
            )],
            scroll: 0.0,
        };
        let at = |x: f32| map.hit([x, 4.0]);
        assert_eq!((at(21.0), at(22.0)), (Some(0), Some(1)), "P");
        assert_eq!((at(26.0), at(27.0)), (Some(1), Some(2)), "l");
        assert_eq!((at(131.0), at(132.0)), (Some(3), Some(4)), "k");
        assert_eq!((at(135.0), at(136.0)), (Some(4), Some(5)), "the space");
        assert_eq!((at(142.0), at(143.0)), (Some(5), Some(6)), "w");
        assert_eq!((at(3.0), at(300.0)), (Some(0), Some(6)), "past either end");
        assert_eq!(at(131.02), Some(3), "a hair past a whole pixel");
    }

    #[test]
    fn the_scroll_moves_only_to_keep_the_caret_in() {
        assert_eq!(scroll_to(0.0, 150.0, [10.0, 110.0]), 41.0);
        assert_eq!(scroll_to(41.0, 100.0, [10.0, 110.0]), 41.0);
        assert_eq!(scroll_to(41.0, 30.0, [10.0, 110.0]), 20.0);
        assert_eq!(scroll_to(41.0, 10.0, [10.0, 110.0]), 0.0);
    }
}
