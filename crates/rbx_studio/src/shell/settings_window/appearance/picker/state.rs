//! The popover's state: what it picks for, the colour as hue, saturation
//! and value, and what applying it would store.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::accent::{self, Check, Status};

use super::super::super::SettingsWindow;

/// What the popover picks a colour for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::settings_window) enum Target {
    Accent,
    /// A transform tool, by its key (`"move"`).
    Tool(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Drag {
    Square,
    Bar,
}

pub(in crate::shell::settings_window) struct Picker {
    pub(in crate::shell::settings_window) target: Target,
    pub(super) hue: f32,
    pub(super) saturation: f32,
    pub(super) value: f32,
    pub(super) hex: Entity<InputState>,
    /// Top-left, in window coordinates.
    pub(super) anchor: Point<Pixels>,
    pub(super) square: Rc<Cell<Bounds<Pixels>>>,
    pub(super) bar: Rc<Cell<Bounds<Pixels>>>,
    pub(super) drag: Option<Drag>,
    _typed: Subscription,
}

impl Picker {
    pub(in crate::shell::settings_window) fn new(
        target: Target,
        start: Rgba,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) -> Self {
        let (hue, saturation, value) = accent::hsv(start);
        let hex = cx.new(|cx| InputState::new(window, cx).default_value(accent::hex(start)));
        let typed = cx.subscribe(&hex, |this, input, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let Some(color) = accent::parse_hex(&input.read(cx).value()) else {
                return;
            };
            if let Some(picker) = &mut this.picker {
                (picker.hue, picker.saturation, picker.value) = accent::hsv(color);
                cx.notify();
            }
        });
        Picker {
            target,
            hue,
            saturation,
            value,
            hex,
            anchor,
            square: Rc::new(Cell::new(Bounds::default())),
            bar: Rc::new(Cell::new(Bounds::default())),
            drag: None,
            _typed: typed,
        }
    }

    pub(in crate::shell::settings_window) fn candidate(&self) -> Rgba {
        accent::from_hsv(self.hue, self.saturation, self.value)
    }

    /// Moves the picked point to `position`, clamped to whichever of the
    /// square or the bar is being dragged, and puts the hex in step.
    pub(super) fn drag_to(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let fraction = |along: Pixels, start: Pixels, size: Pixels| {
            (f32::from(along - start) / f32::from(size).max(1.)).clamp(0., 1.)
        };
        match self.drag {
            Some(Drag::Square) => {
                let b = self.square.get();
                self.saturation = fraction(position.x, b.origin.x, b.size.width);
                self.value = 1. - fraction(position.y, b.origin.y, b.size.height);
            }
            Some(Drag::Bar) => {
                let b = self.bar.get();
                self.hue = fraction(position.x, b.origin.x, b.size.width).min(0.9999);
            }
            None => return,
        }
        let hex = accent::hex(self.candidate());
        self.hex
            .update(cx, |state, cx| state.set_value(hex, window, cx));
    }

    /// The bars this colour is measured against.
    pub(super) fn checks(&self) -> Vec<Check> {
        match self.target {
            Target::Accent => accent::checks(self.candidate()).to_vec(),
            Target::Tool(_) => vec![accent::tool_check(self.candidate())],
        }
    }

    pub(super) fn status(&self) -> Option<Status> {
        match self.target {
            Target::Accent => accent::near_status(self.candidate()),
            Target::Tool(_) => None,
        }
    }

    /// The colour Apply would store: the candidate, or the nearest lighter
    /// one that passes when the candidate fails a bar.
    pub(super) fn applied(&self) -> Option<Rgba> {
        let candidate = self.candidate();
        match self.target {
            Target::Accent => match accent::fix(candidate) {
                Some(fixed) => Some(fixed),
                None if accent::passes(candidate) => Some(candidate),
                None => None,
            },
            Target::Tool(_) => {
                let passes = |c: Rgba| accent::tool_check(c).passes();
                match accent::lighten_until(candidate, passes) {
                    Some(fixed) => Some(fixed),
                    None if passes(candidate) => Some(candidate),
                    None => None,
                }
            }
        }
    }
}
