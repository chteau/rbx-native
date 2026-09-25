//! Drawing the docks around the document: an edge, the docks stacked on
//! it, their tab strips, and — while a drag is in flight — the targets a
//! drop can land on.
//!
//! Every target here is a real element sitting where the panel would
//! actually go, rather than a rectangle computed from pointer coordinates.
//! That is what makes the overlay honest: the thing that lights up *is*
//! the thing that takes the drop, so the promise and the result cannot
//! drift apart, and "half the dock" needs no arithmetic — it is a child
//! filling half of it.
//!
//! An edge holding nothing grows a **ghost dock** while a drag is in
//! flight, the width the dock will be, easing open and shut. It is a
//! child of the same row the real docks are in rather than an overlay
//! floating above them, which is why no two targets can ever overlap.

use std::time::Duration;

use gpui_kit::base::{transition, Transition};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::chrome;
use super::dock_drag::DraggedPanel;
use super::layout::{Edge, Landing, Panel};
use super::Shell;

/// How long a ghost dock takes to open or shut. Short enough not to lag
/// the pointer, long enough to read as the room being made rather than a
/// column appearing from nowhere. Honours the editor's Reduce Motion
/// setting for free — `transition` checks it.
const GHOST: Duration = Duration::from_millis(140);

impl Shell {
    /// One edge: its resize handle and the docks stacked on it.
    ///
    /// Returns nothing at all for an edge nobody put a panel on and no
    /// drag to offer it to — no column, no handle, no hairline — so
    /// emptying an edge gives the room back to the document rather than
    /// leaving a seam where a dock was.
    pub(super) fn dock_edge(
        &mut self,
        edge: Edge,
        limit: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        // A dock whose every tab the document has set aside is left out
        // whole, indices and all intact, so a drop still names the real one.
        let shown: Vec<usize> = (0..self.layout.groups(edge).len())
            .filter(|&index| self.dock_showing(edge, index).is_some())
            .collect();
        if shown.is_empty() {
            return self.ghost_dock(edge, window, cx);
        }

        // Output collapses to its own strip, and only while it is the tab
        // showing in the edge's only dock: a second dock beside it would
        // still need the height, and showing another tab — the Viewport
        // dock it shares a strip with by default — opens the edge again.
        let collapsed = self.output_collapsed
            && shown.len() == 1
            && self.dock_showing(edge, shown[0]) == Some(Panel::Output);
        let size = self.layout.capped(edge, limit);

        let docks: Vec<AnyElement> = shown
            .into_iter()
            .map(|index| self.dock_group(edge, index, collapsed, window, cx))
            .collect();

        let column = edge_column(edge, size, collapsed)
            .children(docks)
            .into_any_element();

        // The handle sits between the edge and the document, so which side
        // of the column it goes on is which edge this is. A collapsed
        // Output has nothing to resize.
        let resize = (!collapsed).then(|| self.handle(edge, cx));
        match edge {
            Edge::Left => [Some(column), resize].into_iter().flatten().collect(),
            _ => [resize, Some(column)].into_iter().flatten().collect(),
        }
    }

    /// One dock on an edge: its tab strip, whichever tab is showing, and
    /// the two halves a drop can split it on.
    fn dock_group(
        &mut self,
        edge: Edge,
        index: usize,
        collapsed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = &self.layout.groups(edge)[index];
        let panels = group.panels().to_vec();
        let Some(active) = self.dock_showing(edge, index) else {
            return div().into_any_element();
        };
        let hidden = self.hidden_panels();
        let solo = panels
            .iter()
            .filter(|panel| !hidden.contains(panel))
            .count()
            <= 1;

        let tabs: Vec<AnyElement> = panels
            .iter()
            .enumerate()
            .filter(|(_, panel)| !hidden.contains(panel))
            .map(|(tab, panel)| {
                let title = self.panel_title(*panel);
                self.dock_tab(*panel, title, edge, index, tab, *panel == active, solo, cx)
            })
            .collect();
        let (trailing, content) = self.panel_parts(active, collapsed, window, cx);
        let splits = self.split_targets(edge, index, cx);

        v_flex()
            .flex_1()
            .overflow_hidden()
            // A seam between two docks sharing an edge: flush against one
            // another they read as one dock with two headers.
            .when(index > 0, |this| {
                if edge.is_vertical() {
                    this.border_t(px(1.)).border_color(tokens::border())
                } else {
                    this.border_l(px(1.)).border_color(tokens::border())
                }
            })
            .child(self.tab_strip(edge, index, tabs, trailing, active.has_toolbar(), cx))
            // The split halves cover the content only, never the strip: a
            // later-painted overlay takes the drop first, so halves laid
            // over the whole dock would swallow every drop aimed at a tab.
            .child(
                v_flex()
                    .relative()
                    .flex_1()
                    .overflow_hidden()
                    .children(content)
                    .children(splits),
            )
            .into_any_element()
    }

    /// The strip itself, which is also the target that means "as a tab of
    /// this dock" — aimed at past the last tab rather than at one of them.
    fn tab_strip(
        &self,
        edge: Edge,
        group: usize,
        tabs: Vec<AnyElement>,
        trailing: Option<AnyElement>,
        toolbar: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let handle = cx.entity();
        let landing = Landing::Tab {
            edge,
            group,
            tab: tabs.len(),
        };

        div()
            .id(SharedString::from(format!(
                "dock-strip-{}-{group}",
                edge.label()
            )))
            .w_full()
            .flex_none()
            .drag_over::<DraggedPanel>(|style, _, _, _| style.bg(tokens::check_on().opacity(0.22)))
            .on_drop(move |dragged: &DraggedPanel, _, cx| {
                let panel = dragged.0;
                handle.update(cx, |shell, cx| shell.land_panel(panel, landing, cx));
            })
            .child(chrome::dock_strip(tabs, trailing, toolbar))
            .into_any_element()
    }

    /// The two halves of a dock that mean "a new dock before/after this
    /// one", drawn over its content only while a drag is in flight.
    ///
    /// This is the choice the gesture has to offer and a single whole-dock
    /// target cannot: landing on the strip joins the dock, landing on a
    /// half splits the edge. Each half lights up at the size the new dock
    /// will actually be, because it *is* that size.
    fn split_targets(&self, edge: Edge, index: usize, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.dragging_panel.is_none() {
            return Vec::new();
        }

        [(false, index), (true, index + 1)]
            .into_iter()
            .map(|(second, group)| {
                let handle = cx.entity();
                let landing = Landing::NewGroup { edge, group };
                div()
                    .id(SharedString::from(format!(
                        "dock-split-{}-{index}-{second}",
                        edge.label()
                    )))
                    .absolute()
                    // Docks stack down a side edge and across the bottom
                    // one, so the half a drop splits on follows the same
                    // axis.
                    .map(|this| match (edge.is_vertical(), second) {
                        (true, false) => this.left_0().right_0().top_0().h(relative(0.5)),
                        (true, true) => this.left_0().right_0().bottom_0().h(relative(0.5)),
                        (false, false) => this.top_0().bottom_0().left_0().w(relative(0.5)),
                        (false, true) => this.top_0().bottom_0().right_0().w(relative(0.5)),
                    })
                    .drag_over::<DraggedPanel>(|style, _, _, _| {
                        style
                            .bg(tokens::check_on().opacity(0.28))
                            .border_color(tokens::check_on())
                    })
                    .border_2()
                    .border_color(tokens::check_on().opacity(0.18))
                    .on_drop(move |dragged: &DraggedPanel, _, cx| {
                        let panel = dragged.0;
                        handle.update(cx, |shell, cx| shell.land_panel(panel, landing, cx));
                    })
                    .into_any_element()
            })
            .collect()
    }

    /// The room an empty edge offers while a drag is in flight: a column
    /// the width the dock will be, easing open when the drag starts and
    /// shut when it ends.
    ///
    /// A child of the row rather than an overlay above it, which is what
    /// keeps the three edges' targets from ever overlapping — they are
    /// siblings in a flex row, so they cannot share a pixel.
    fn ghost_dock(
        &self,
        edge: Edge,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let open = self.dragging_panel.is_some();
        let target = if open { self.layout.size(edge) } else { 0. };
        let size = transition(
            ("ghost-dock", edge.label()),
            px(target),
            Transition::new(GHOST),
            window,
            cx,
        );
        if size <= px(0.5) {
            return Vec::new();
        }

        let handle = cx.entity();
        let landing = Landing::NewGroup { edge, group: 0 };
        let zone = div()
            .id(SharedString::from(format!("ghost-{}", edge.label())))
            .flex_none()
            .map(|this| {
                if edge.is_vertical() {
                    this.w(size).h_full()
                } else {
                    this.h(size).w_full()
                }
            })
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
                handle.update(cx, |shell, cx| shell.land_panel(panel, landing, cx));
            })
            .into_any_element();
        vec![zone]
    }

    /// One tab: shows its panel on a press, carries it on a drag, takes a
    /// drop to land beside itself, and closes on its own button.
    ///
    /// The tab is the grab handle for the whole dock, which is the gesture
    /// every editor with movable panels uses — and the reason the panel's
    /// own name had to become data (see `shell::layout`) rather than the
    /// function that drew it.
    #[allow(clippy::too_many_arguments)]
    fn dock_tab(
        &self,
        panel: Panel,
        title: SharedString,
        edge: Edge,
        group: usize,
        tab: usize,
        selected: bool,
        solo: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let show = cx.entity();
        let start = cx.entity();
        let drop = cx.entity();
        let shut = cx.entity();
        let landing = Landing::Tab { edge, group, tab };

        chrome::dock_tab(panel.key(), title, selected, solo, move |_, _, cx| {
            shut.update(cx, |shell, cx| shell.close_panel(panel, cx));
        })
        // Mouse-*down*, not click: this element is also the drag handle,
        // and a press that goes on to move is a drag whose click never
        // arrives. Showing the tab on the press is what the Explorer's own
        // rows do for the same reason, and it is the more responsive half
        // of the bargain anyway.
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            show.update(cx, |shell, cx| shell.activate_panel(panel, cx));
        })
        .on_drag(DraggedPanel(panel), move |dragged, _, _, cx| {
            start.update(cx, |shell, cx| shell.begin_panel_drag(dragged.0, cx));
            cx.new(|_| dragged.clone())
        })
        .drag_over::<DraggedPanel>(|style, _, _, _| style.bg(tokens::check_on().opacity(0.35)))
        // Landing *on* a tab inserts at its position, which is what makes
        // a strip reorderable rather than append-only.
        .on_drop(move |dragged: &DraggedPanel, _, cx| {
            let carried = dragged.0;
            drop.update(cx, |shell, cx| shell.land_panel(carried, landing, cx));
        })
        .into_any_element()
    }

    /// The tab a dock shows: its own active one, or — while the document
    /// has set that one aside (see `Shell::hidden_panels`) — the first of
    /// the rest. `None` when every tab it holds is set aside.
    fn dock_showing(&self, edge: Edge, index: usize) -> Option<Panel> {
        let group = self.layout.groups(edge).get(index)?;
        let hidden = self.hidden_panels();
        group
            .active()
            .filter(|active| !hidden.contains(active))
            .or_else(|| {
                group
                    .panels()
                    .iter()
                    .copied()
                    .find(|panel| !hidden.contains(panel))
            })
    }

    /// What a panel's tab reads.
    fn panel_title(&self, panel: Panel) -> SharedString {
        match panel {
            // Named after the instance it is showing, which is what the
            // dock's own title said before there were tabs.
            Panel::Properties => self.properties_title(),
            other => SharedString::from(other.key()),
        }
    }
}

/// The column one edge's docks are stacked in.
fn edge_column(edge: Edge, size: f32, collapsed: bool) -> Div {
    let column = if edge.is_vertical() {
        v_flex().flex_none().h_full().w(px(size))
    } else {
        // Docks stack *across* the bottom edge rather than down it: it is
        // the wide one, and two half-height strips under the document
        // would leave neither usable.
        h_flex()
            .flex_none()
            .w_full()
            // Stretched, not `h_flex`'s centring: a dock shorter than the
            // edge would float in the middle of it, with dead space between
            // the document and its tab strip.
            .items_stretch()
            .when(!collapsed, |this| this.h(px(size)))
    }
    .bg(tokens::dock());

    match edge {
        Edge::Left => column.border_r(px(1.)),
        Edge::Right => column.border_l(px(1.)),
        Edge::Bottom => column.border_t(px(1.)),
    }
    .border_color(tokens::border())
}
