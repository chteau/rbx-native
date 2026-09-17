//! The two text properties that change an element's geometry, `TextScaled`
//! and `AutomaticSize`, both of which need the text measured — and layout is
//! pure CPU with no font system of its own. [`TextMeasure`] is the hole the
//! renderer fills with its cosmic-text-backed typesetter; a test fills it
//! with a fixed advance per character.

use super::super::plan::Text;
use super::Rect;

/// `TextScaled`'s search range: Roblox's docs put the ceiling at 100 with no
/// size constraint, and a text size below one pixel draws nothing anyway.
const MIN_SIZE: f32 = 1.0;
const MAX_SIZE: f32 = 100.0;

/// Measures a text the way it will be drawn.
pub(crate) trait TextMeasure {
    /// The pixel bounds of `text` laid out at `size` — width of the widest
    /// line, height of every line — wrapped at `max_width` where given.
    fn measure(&mut self, text: &Text, size: f32, max_width: Option<f32>) -> [f32; 2];
}

/// What `resolve` uses when no typesetter is handed in: a tree with no text
/// never measures anything, and one with text draws it at `TextSize` as is.
#[cfg(test)]
pub(super) struct Unmeasured;

#[cfg(test)]
impl TextMeasure for Unmeasured {
    fn measure(&mut self, _text: &Text, _size: f32, _max_width: Option<f32>) -> [f32; 2] {
        [0.0, 0.0]
    }
}

/// A text at the size it is drawn at, inside the element's `rect`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Typeset {
    pub(crate) text: Text,
    /// The pixel size of one line: `TextSize`, or what `TextScaled` settled
    /// on, either clamped by `Text::size_bounds`.
    pub(crate) size: f32,
}

/// The width the text wraps at inside `rect`, `None` where it runs free —
/// which an `AutomaticSize` on the X axis makes it, since the box then takes
/// its width from the text rather than the other way round.
fn wrap_width(text: &Text, rect: &Rect) -> Option<f32> {
    ((text.wrapped || text.scaled) && !text.automatic[0]).then_some(rect.width)
}

/// Settles the size the text is drawn at and grows `rect` along its automatic
/// axes to hold it.
pub(super) fn typeset(text: &Text, rect: &mut Rect, measure: &mut dyn TextMeasure) -> Typeset {
    let (min, max) = text.size_bounds.unwrap_or((MIN_SIZE, MAX_SIZE));
    let size = match text.scaled {
        true => fit(text, rect, measure, min.max(MIN_SIZE), max),
        false => match text.size_bounds {
            Some((min, max)) => text.size.clamp(min, max),
            None => text.size,
        },
    };
    if text.automatic.iter().any(|axis| *axis) {
        let bounds = measure.measure(text, size, wrap_width(text, rect));
        if text.automatic[0] {
            rect.width = rect.width.max(bounds[0]);
        }
        if text.automatic[1] {
            rect.height = rect.height.max(bounds[1]);
        }
    }
    Typeset {
        text: text.clone(),
        size,
    }
}

/// The largest whole pixel size in `min..=max` at which the wrapped text still
/// fits the box. Bounds only grow with size, so a binary search finds it in a
/// handful of measurements — each of which shapes the whole string.
fn fit(text: &Text, rect: &Rect, measure: &mut dyn TextMeasure, min: f32, max: f32) -> f32 {
    // Half a pixel of slack: a line whose advance rounds to the box's width
    // fits it as far as anyone looking can tell.
    let fits = |measure: &mut dyn TextMeasure, size: f32| {
        let bounds = measure.measure(text, size, wrap_width(text, rect));
        bounds[0] <= rect.width + 0.5 && bounds[1] <= rect.height + 0.5
    };
    let (mut low, mut high) = (min.ceil() as i32, max.floor() as i32);
    let mut best = low;
    while low <= high {
        let middle = (low + high) / 2;
        match fits(measure, middle as f32) {
            true => {
                best = middle;
                low = middle + 1;
            }
            false => high = middle - 1,
        }
    }
    best as f32
}
