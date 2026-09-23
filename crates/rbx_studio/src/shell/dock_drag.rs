//! Dragging a dock by its tab: what travels, and what ends the gesture.
//!
//! GPUI performs the drag itself, the same way `shell::reparent` already
//! has the Explorer do it: `on_drag` on the tab a drag starts from,
//! `on_drop` on whatever is under the cursor, `drag_over` for the tint in
//! between. Where a drop may *land* is `shell::docks`' business — every
//! target there is a real element sitting where the panel would go — so
//! nothing here computes a rectangle or tracks a pointer.
//!
//! What is left for this module is the one case GPUI cannot report: a
//! pointer released **outside the window** is over nothing of ours, so no
//! drop ever arrives for it. That is a tear-out, and it is decided on
//! mouse-up in [`Shell::end_panel_drag`].

use gpui_kit::*;

use super::layout::Panel;
use super::Shell;

/// The panel a drag is carrying.
///
/// GPUI wants an entity it can render as the thing under the cursor, so
/// this is both the payload and its own ghost.
#[derive(Clone)]
pub(super) struct DraggedPanel(pub(super) Panel);

impl Render for DraggedPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The tab itself, travelling: a ghost that looked like anything
        // else would leave you guessing what you had picked up.
        super::chrome::dock_tab(
            "dragged",
            SharedString::from(self.0.key()),
            true,
            false,
            |_, _, _| {},
        )
        .opacity(0.8)
        .into_any_element()
    }
}

impl Shell {
    /// Records that a tab has been picked up, which is what puts the ghost
    /// docks and the split halves on screen (see `shell::docks`).
    pub(super) fn begin_panel_drag(&mut self, panel: Panel, cx: &mut Context<Self>) {
        self.dragging_panel = Some(panel);
        cx.notify();
    }

    /// Ends a drag that no target claimed.
    ///
    /// Let go over the document and the dock stays where it was — the
    /// middle of the window means "not a drop". Let go **outside the
    /// window** and it tears out into a window of its own, which is the
    /// gesture every editor with floating panels uses and the only one
    /// GPUI cannot report as a drop, since there is nothing of ours under
    /// the cursor to report it to.
    pub(super) fn end_panel_drag(
        &mut self,
        position: Point<Pixels>,
        bounds: Size<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.dragging_panel.take() else {
            return;
        };

        let outside = position.x < px(0.)
            || position.y < px(0.)
            || position.x > bounds.width
            || position.y > bounds.height;
        if outside {
            self.float_panel(panel, cx);
        }
        cx.notify();
    }
}
