//! Dragging a dock by its tab: what travels, where it may land, and the
//! overlay that says so.
//!
//! GPUI performs the gesture, the same way `shell::reparent` already has
//! the Explorer do it: `on_drag` on the tab a drag starts from, `on_drop`
//! on whatever is under the cursor, `drag_over` for the tint in between.
//! Nothing here tracks pointer positions itself.
//!
//! Two things this has that a row-onto-row drag does not. A dock can be
//! dropped on an edge that currently holds **nothing**, so there is no
//! element there to drop on — hence the [`Shell::drop_zones`] overlay,
//! three strips that exist only while a drag is in flight. And a dock can
//! be dropped **outside the window entirely**, which is a tear-out; that
//! one is decided on mouse-up (see `Shell::end_panel_drag`), because a
//! drop that lands on nothing is exactly what GPUI does not report.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::layout::{Edge, Panel};
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
        super::chrome::dock_tab("dragged", SharedString::from(self.0.key()), true)
            .opacity(0.8)
            .into_any_element()
    }
}

/// How far into the window an edge's drop strip reaches. Wide enough to
/// hit without aiming, narrow enough that the document keeps a middle that
/// means "not here".
const ZONE: f32 = 72.;

impl Shell {
    /// The three strips a dragged dock can be dropped on, drawn over the
    /// row only while a drag is actually in flight.
    ///
    /// They have to exist separately from the dock columns because an edge
    /// holding nothing has no column to aim at — and an edge you can empty
    /// but never refill is a trap. Drawn *over* the docks rather than
    /// beside them for the same reason: the left strip has to accept a
    /// drop even when a dock already covers that part of the window.
    pub(super) fn drop_zones(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.dragging_panel.is_none() {
            return Vec::new();
        }

        Edge::ALL
            .into_iter()
            .map(|edge| {
                let handle = cx.entity();
                div()
                    .id(SharedString::from(format!("drop-{}", edge.label())))
                    .absolute()
                    .map(|this| match edge {
                        Edge::Left => this.left_0().top_0().bottom_0().w(px(ZONE)),
                        Edge::Right => this.right_0().top_0().bottom_0().w(px(ZONE)),
                        Edge::Bottom => this.left_0().right_0().bottom_0().h(px(ZONE)),
                    })
                    // Visible before it is hovered, not only during: a drop
                    // target nobody can see until they have already aimed
                    // at it is one they never find.
                    .bg(tokens::check_on().opacity(0.12))
                    .border_2()
                    .border_color(tokens::check_on().opacity(0.35))
                    .drag_over::<DraggedPanel>(|style, _, _, _| {
                        style
                            .bg(tokens::check_on().opacity(0.28))
                            .border_color(tokens::check_on())
                    })
                    .on_drop(move |dragged: &DraggedPanel, _, cx| {
                        let panel = dragged.0;
                        handle.update(cx, |shell, cx| {
                            shell.dragging_panel = None;
                            shell.dock_panel(panel, edge, None, cx);
                        });
                    })
                    .into_any_element()
            })
            .collect()
    }

    /// Records that a tab has been picked up, so [`Self::drop_zones`] has
    /// something to draw.
    pub(super) fn begin_panel_drag(&mut self, panel: Panel, cx: &mut Context<Self>) {
        self.dragging_panel = Some(panel);
        cx.notify();
    }

    /// Ends a drag that no drop zone claimed.
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
