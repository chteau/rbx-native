//! Drag a numeric field's label sideways to change its value.
//!
//! The gesture every 3D tool has, and for the same reason: nudging a
//! position by eye is a scrub, not a typing exercise. Roblox Studio,
//! Blender, Unity and Figma all bind it to a horizontal drag on the field's
//! *label* rather than the field itself, which is what keeps a plain click
//! meaning "put the caret here and type".
//!
//! How far a pixel moves the value is a property of the **field**, not of
//! the row: a `UDim` is a decimal scale beside an integer offset, and
//! dragging the offset should step through whole units while the scale
//! moves in hundredths. `properties::FieldKind::step_per_pixel` owns that,
//! and a `Text` field (a `Font`'s family name) has no step at all and can't
//! be dragged.
//!
//! The scrub writes through the field's own `InputState` rather than
//! straight to the DOM. That is deliberate: the input already has a
//! change subscription that parses, validates and commits (see
//! `shell::edit`), so a drag takes exactly the path typing does — including
//! the rounding and clamping a type needs — instead of a second, parallel
//! one that could disagree with it.

use gpui_kit::{Modifiers, Pixels};

use crate::properties::FieldKind;

/// A drag in progress on one field.
#[derive(Debug, Clone)]
pub(super) struct Scrub {
    /// The property whose row is being dragged, and which of its fields.
    pub(super) property: String,
    pub(super) index: usize,
    pub(super) kind: FieldKind,
    /// Where the pointer went down, and the value at that moment.
    ///
    /// Both, rather than a delta accumulated per frame: a long drag that
    /// accumulates would drift, and one that leaves the window and comes
    /// back would jump.
    pub(super) origin: Pixels,
    pub(super) start: f32,
}

impl Scrub {
    /// The value this pointer position means.
    pub(super) fn value_at(&self, x: Pixels, modifiers: Modifiers) -> Option<f32> {
        let step = self.kind.step_per_pixel()? * precision(modifiers);
        let moved = self.start + f32::from(x - self.origin) * step;

        Some(match self.kind {
            FieldKind::Integer => moved.round(),
            // Three decimals is what the panel prints; keeping more would
            // put `0.30000001` in a field nobody typed into.
            FieldKind::Decimal => (moved * 1000.).round() / 1000.,
            FieldKind::Text => return None,
        })
    }
}

/// Shift coarsens, Alt refines — the convention every tool with this gesture
/// shares, so it needs no discovering.
pub(super) fn precision(modifiers: Modifiers) -> f32 {
    match (modifiers.shift, modifiers.alt) {
        (true, false) => 10.,
        (false, true) => 0.1,
        _ => 1.,
    }
}

/// A value as the field should read it back.
///
/// An integer never shows a decimal point, and a decimal never shows
/// trailing zeros — a field that reads `4.000` after a drag looks like it
/// has more precision than it does.
pub(super) fn format(value: f32, kind: FieldKind) -> String {
    match kind {
        FieldKind::Integer => format!("{}", value as i64),
        FieldKind::Decimal | FieldKind::Text => {
            let text = format!("{value:.3}");
            let text = text.trim_end_matches('0').trim_end_matches('.');
            if text.is_empty() || text == "-" {
                "0".to_owned()
            } else {
                text.to_owned()
            }
        }
    }
}

#[cfg(test)]
#[path = "scrub/tests.rs"]
mod tests;

impl super::Shell {
    /// Starts a scrub from wherever the field currently stands.
    ///
    /// The starting value is read out of the field's own text rather than
    /// out of the DOM: the two can differ mid-edit, and a drag that snapped
    /// back to the committed value the moment it began would be maddening.
    pub(super) fn begin_scrub(
        &mut self,
        property: &str,
        index: usize,
        kind: FieldKind,
        origin: Pixels,
        cx: &mut gpui_kit::App,
    ) {
        if kind.step_per_pixel().is_none() {
            return;
        }
        let Some(start) = self
            .field_input(property, index)
            .and_then(|input| input.read(cx).value().trim().parse::<f32>().ok())
        else {
            return;
        };

        self.scrub = Some(Scrub {
            property: property.to_owned(),
            index,
            kind,
            origin,
            start,
        });
    }

    /// Applies an in-progress scrub.
    pub(super) fn drag_scrub(
        &mut self,
        x: Pixels,
        modifiers: Modifiers,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::App,
    ) {
        let Some(scrub) = self.scrub.clone() else {
            return;
        };
        let Some(value) = scrub.value_at(x, modifiers) else {
            return;
        };
        let Some(input) = self.field_input(&scrub.property, scrub.index) else {
            return;
        };

        let text = format(value, scrub.kind);
        if input.read(cx).value() == text.as_str() {
            return;
        }
        // Writing the field is the whole commit: its change subscription
        // parses and writes through to the DOM (see `shell::edit`), so a
        // drag and a typed value take exactly the same path.
        input.update(cx, |state, cx| state.set_value(text, window, cx));
    }
}
