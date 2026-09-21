//! Studio's measurement box after a Move-arrow drag
//! (`MoveHandles:_renderPassiveMoveMeasurement`, `FloatingValueInput`): the
//! distance label a drag showed stays where it was once the button comes up,
//! and becomes editable. A distance typed into it and entered puts the
//! selection exactly that far from where the drag started, along the arrow
//! it held — one undo step per entry. The box stays until the next press in
//! the view, a selection change or a tool switch.
//!
//! Studio's text box leaves Enter alone: it commits a number, reverts
//! anything else, and releases the keyboard either way. Losing focus any
//! other way — a click elsewhere, `Escape` — commits nothing and leaves the
//! typed text as it is.

use glam::Vec3;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;
use rbx_viewer::gizmo::Axis;

use super::{guides, ViewportAction, WorkspaceView};
use crate::dragger::label;

/// Studio's `BorderSelected` and `BorderHover`, dark theme.
const BORDER_LIT: u32 = 0x008bea;

/// The box, while it is up.
pub(super) struct Measure {
    input: Entity<InputState>,
    /// The Move arrow the drag held — its axis, which end (`±1`) and how far
    /// out the press landed — which the box stands beside.
    arrow: (Axis, f32, f32),
    /// The arrow's direction at the press, which a typed distance runs along.
    direction: Vec3,
    /// How far the selection stands from where the drag started, along
    /// `direction` (Studio's `_lastDelta`): what the box reads.
    moved: f32,
    hovered: bool,
    _entered: Subscription,
}

impl WorkspaceView {
    /// A Move-arrow drag let go: its label stays, editable, reading how far
    /// the drag went — `0` for an arrow pressed and let go. Nothing with the
    /// measurement setting off, as nothing was shown during the drag.
    pub(super) fn open_measure(
        &mut self,
        arrow: (Axis, f32, f32),
        direction: Vec3,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.guides.settings.show_measurement {
            return;
        }
        let moved = self
            .targets
            .anchor()
            .zip(self.held.anchor())
            .map_or(0.0, |(now, then)| {
                (now.position() - then.position()).dot(direction)
            });
        let input = cx.new(|cx| InputState::new(window, cx).default_value(label::concise(moved)));
        let entered = cx.subscribe_in(&input, window, |view, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::PressEnter { .. } => view.enter_measure(window, cx),
                // The box grows with what is typed in it.
                InputEvent::Change => cx.notify(),
                _ => {}
            }
        });
        self.measure = Some(Measure {
            input,
            arrow,
            direction,
            moved,
            hovered: false,
            _entered: entered,
        });
    }

    /// Takes the box down.
    pub(super) fn close_measure(&mut self) {
        self.measure = None;
    }

    /// Enter in the box: a number moves the selection to that distance from
    /// the drag's start (`_doMeasuredMove`), from wherever it stands now;
    /// anything else puts the box back to what it read. The keyboard goes
    /// back to the view either way.
    fn enter_measure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(measure) = self.measure.as_mut() else {
            return;
        };
        let text = measure.input.read(cx).value();
        if let Some((step, distance)) = entered(measure.moved, &text) {
            measure.moved = distance;
            let moves = self.targets.translate(measure.direction * step);
            cx.emit(ViewportAction::Moved {
                moves,
                first: true,
                settle: None,
            });
        }
        let Some(measure) = self.measure.as_ref() else {
            return;
        };
        let reads = label::concise(measure.moved);
        measure
            .input
            .update(cx, |state, cx| state.set_value(reads, window, cx));
        window.focus(&self.focus, cx);
    }

    /// Whether the box has the keyboard: every key then belongs to it, not
    /// to the camera or the tool shortcuts.
    pub(super) fn typing(&self, window: &Window, cx: &App) -> bool {
        self.measure
            .as_ref()
            .is_some_and(|measure| measure.input.focus_handle(cx).is_focused(window))
    }

    /// The box, beside the arrow where it stands now — it follows the
    /// handles and the camera — or nothing when that spot is off screen.
    pub(super) fn measure_element(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let measure = self.measure.as_ref()?;
        let handles = self.handles()?;
        let (axis, sign, along) = measure.arrow;
        let at = self.move_label(&handles, axis, sign, along, window.scale_factor())?;
        let lit = measure.hovered || self.typing(window, cx);
        let input = measure.input.clone();
        // Studio's box grows with what is typed in it; a bold digit is
        // about 0.6 of the text's size across.
        let chars = input.read(cx).value().chars().count().max(1) as f32;
        let text = guides::label_text();
        let field = Input::new(&input)
            .appearance(false)
            .p_0()
            .w(text * (0.6 * chars) + px(4.0))
            .h(text)
            .text_size(text)
            .line_height(text)
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0xffffff));
        let border = if lit { BORDER_LIT } else { 0x000000 };
        let boxed = guides::measurement_box(rgb(border))
            .child(field)
            .id("measurement-box")
            // Studio's box takes the click: the view never sees it,
            // so it neither closes the box nor starts a pick.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Every click in it selects the whole number, so typing
            // replaces it.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |_, _: &MouseUpEvent, window, cx| {
                    input.update(cx, |state, cx| state.select_all(window, cx));
                }),
            )
            .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                if let Some(measure) = view.measure.as_mut() {
                    measure.hovered = *hovered;
                    cx.notify();
                }
            }));
        Some(guides::centred_on(at, boxed).into_any_element())
    }
}

/// What Enter does with `text` in the box, the selection standing `moved`
/// from where the drag started: how far to step along the arrow from where
/// it stands now, and the distance the box then reads. `None` for text that
/// is no number, which moves nothing. Entering the distance already read
/// still steps, by nothing, as Studio's does.
fn entered(moved: f32, text: &str) -> Option<(f32, f32)> {
    let distance = label::typed(text)?;
    Some((distance - moved, distance))
}

#[cfg(test)]
mod tests {
    use super::entered;

    #[test]
    fn a_typed_distance_is_measured_from_the_drags_start() {
        // Dragged 3 out: typing 5 steps 2 more, typing -1 steps 4 back.
        assert_eq!(entered(3.0, "5"), Some((2.0, 5.0)));
        assert_eq!(entered(3.0, " -1 "), Some((-4.0, -1.0)));
        assert_eq!(entered(3.0, "3"), Some((0.0, 3.0)));
    }

    #[test]
    fn text_that_is_no_number_moves_nothing() {
        assert_eq!(entered(3.0, "4 studs"), None);
        assert_eq!(entered(3.0, ""), None);
    }
}
