//! The Viewport dock: the graphics quality, the emulated screen, every
//! view setting, and the live frame rate — kept in a dock so that nothing
//! persistent sits over the scene being edited. Four sections (Rendering,
//! Camera, Overlays, Dragging & snapping) as fixed columns when the dock
//! is wide, a 2×2 grid at a middling width, one column when narrow.
//!
//! The settings are the shell's own state and apply whether or not this dock
//! is open; the dock only shows them. The frame-rate sampling is the other
//! way round: it runs only while the dock is on screen (see
//! [`Shell::sync_stats`]), because a readout nobody can see is per-frame
//! work on both the render and the UI thread for nothing.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::pacing::UnfocusedFps;
use crate::tokens;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::Shell;

mod rows;
mod toggles;

/// The four sections' widths in the wide layout, in order.
const COLUMN_WIDTHS: [f32; 4] = [300., 200., 260., 230.];
/// Between two side-by-side sections: 24px, a hairline, 24px.
const COLUMN_GAP: f32 = 24.;
/// Between two stacked sections (or the grid's two rows): 16px, a
/// hairline, 16px.
const STACK_GAP: f32 = 16.;
/// The body's padding: 14/20 in the wide layout, 14 all round otherwise.
const PADDING_Y: f32 = 14.;
const WIDE_PADDING_X: f32 = 20.;
const PADDING: f32 = 14.;
/// Content width from which the four columns fit.
const FOUR_COLUMNS_MIN: f32 = 1100.;
/// Content width from which the sections pair up as a 2×2 grid.
const GRID_MIN: f32 = 560.;

/// How the four sections are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Columns {
    /// Four fixed-width columns side by side.
    Four,
    /// Rendering | Camera over Overlays | Dragging & snapping.
    Grid,
    /// All four stacked.
    One,
}

/// The layout for a dock `width` pixels wide: the wide padding applies
/// only with the four columns; below that, every dock body uses 14px.
pub(super) fn layout_for(width: f32) -> Columns {
    if width - 2. * WIDE_PADDING_X >= FOUR_COLUMNS_MIN {
        Columns::Four
    } else if width - 2. * PADDING >= GRID_MIN {
        Columns::Grid
    } else {
        Columns::One
    }
}

/// One view setting: what its row reads, whether it is on, and what sets it.
type Toggle = (
    &'static str,
    bool,
    fn(&mut Shell, bool, &mut Context<Shell>),
);

/// The dock's own state: the bounds the last frame gave it.
pub(super) struct ViewportDock {
    width: Rc<Cell<f32>>,
    height: Rc<Cell<f32>>,
}

impl ViewportDock {
    pub(super) fn new() -> Self {
        ViewportDock {
            width: Rc::new(Cell::new(1280.)),
            height: Rc::new(Cell::new(0.)),
        }
    }
}

impl Shell {
    fn set_unfocused_cap(&mut self, capped: bool, cx: &mut Context<Self>) {
        let preset = if capped {
            UnfocusedFps::Fps25
        } else {
            UnfocusedFps::Fps30
        };
        self.set_unfocused_fps(preset, cx);
    }

    /// Samples the frame rate while this dock is on screen and not otherwise
    /// — or throughout, when `RBX_STUDIO_STATS=1` asked for the numbers on
    /// stderr. The variable never opens the dock: it speaks for one run, and
    /// a layout it changed would be written back at the next settings save.
    ///
    /// Asked every render rather than at each place the layout changes — a
    /// drop, a tab click, a close, Reset Layout, a torn-out window shut — so
    /// that no way of moving a dock can forget to; when nothing changed it
    /// is one atomic swap.
    pub(super) fn sync_stats(&self, cx: &App) {
        let wanted =
            self.layout.is_showing(Panel::Viewport) || crate::workspace_view::stats_requested();
        self.viewport.read(cx).set_stats_sampling(wanted);
    }

    /// Scrolls the dock just far enough to show the setting keyboard focus
    /// has moved to. The list outgrows a short dock, and End or a wrapping
    /// arrow would otherwise put focus on a row scrolled out of sight
    /// (WCAG 2.4.11). Reads last frame's layout, which a focus move does
    /// not change.
    fn reveal_viewport_setting(&self) {
        let Some(row) = self
            .viewport_rows
            .borrow()
            .get(self.viewport_nav.current())
            .copied()
        else {
            return;
        };
        let view = self.viewport_scroll.bounds();
        let shift = if row.bottom() > view.bottom() {
            row.bottom() - view.bottom()
        } else if row.top() < view.top() {
            row.top() - view.top()
        } else {
            return;
        };
        let offset = self.viewport_scroll.offset();
        self.viewport_scroll
            .set_offset(point(offset.x, offset.y - shift));
    }

    /// The dock's trailing menu and its body.
    ///
    /// The quality select is one Tab stop and the switches are one more:
    /// a roving group, the way the Properties panel's checkboxes are, so
    /// arrows walk the list in reading order and Tab leaves it — not ten
    /// stops to press through on the way to the next dock.
    pub(super) fn viewport_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::ViewportOverflow,
            chrome::Trigger::new(chrome::dock_options_button(
                "viewport-overflow",
                IconName::Ellipsis,
                16.,
                "Viewport dock options",
            )),
            self.move_items(Panel::Viewport),
            cx,
        );

        let columns = layout_for(self.viewport_ui.width.get());
        let [camera, overlays, dragging] = self.viewport_toggles();
        let cap: Toggle = (
            "Cap at 25 fps when unfocused",
            self.unfocused_fps == UnfocusedFps::Fps25,
            Shell::set_unfocused_cap,
        );
        let total = 1 + camera.len() + overlays.len() + dragging.len();
        self.viewport_nav.begin(&self.tab_order, Some(total), cx);
        self.viewport_rows
            .borrow_mut()
            .resize(total, Bounds::default());
        let mut next = 0;
        let mut switches = |shell: &mut Shell, toggles: Vec<Toggle>, cx: &mut Context<Shell>| {
            let first = next;
            next += toggles.len();
            shell.switch_rows(first, toggles, cx)
        };

        let rendering = rows::section(
            "RENDERING",
            vec![
                self.quality_row(window, cx),
                self.screen_row(window, cx),
                self.frame_rate_row(cx),
            ]
            .into_iter()
            .chain(switches(self, vec![cap], cx))
            .collect(),
        );
        let camera = rows::section("CAMERA", switches(self, camera, cx));
        let overlays = rows::section("OVERLAYS", switches(self, overlays, cx));
        let dragging = rows::section("DRAGGING & SNAPPING", switches(self, dragging, cx));

        let sections = match columns {
            Columns::Four => h_flex()
                .flex_1()
                .items_start()
                .gap(px(COLUMN_GAP))
                .child(rows::fixed(rendering, COLUMN_WIDTHS[0]))
                .child(rows::vertical_rule())
                .child(rows::fixed(camera, COLUMN_WIDTHS[1]))
                .child(rows::vertical_rule())
                .child(rows::fixed(overlays, COLUMN_WIDTHS[2]))
                .child(rows::vertical_rule())
                .child(rows::fixed(dragging, COLUMN_WIDTHS[3])),
            Columns::Grid => v_flex()
                .gap(px(STACK_GAP))
                .child(rows::pair(rendering, camera, COLUMN_GAP))
                .child(rows::horizontal_rule())
                .child(rows::pair(overlays, dragging, COLUMN_GAP)),
            Columns::One => v_flex()
                .gap(px(STACK_GAP))
                .child(rendering)
                .child(rows::horizontal_rule())
                .child(camera)
                .child(rows::horizontal_rule())
                .child(overlays)
                .child(rows::horizontal_rule())
                .child(dragging),
        };

        let body = div()
            .id("viewport-dock")
            .size_full()
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.viewport_nav.key(&event.keystroke, window, cx) {
                    shell.reveal_viewport_setting();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .overflow_y_scroll()
            .track_scroll(&self.viewport_scroll)
            .child(
                // At least the dock's height, so the wide layout's rules
                // run to the body's bottom padding as the sections' rows
                // stop short of it; taller content still scrolls.
                div()
                    .min_h(relative(1.))
                    .flex()
                    .flex_col()
                    .py(px(PADDING_Y))
                    .px(px(if columns == Columns::Four {
                        WIDE_PADDING_X
                    } else {
                        PADDING
                    }))
                    .text_size(tokens::text_md())
                    .line_height(tokens::line_md())
                    .child(sections),
            )
            .vertical_scrollbar(&self.viewport_scroll);

        let width = self.viewport_ui.width.clone();
        let height = self.viewport_ui.height.clone();
        let shell = cx.entity_id();
        let measured = div()
            .size_full()
            .flex()
            .flex_col()
            // The size this frame gave the body is what the next frame lays
            // out for. When it changed, ask for that next frame now, so a
            // resize settles in one extra pass rather than waiting for the
            // next unrelated redraw.
            .on_children_prepainted(move |bounds, window, cx| {
                if let Some(bounds) = bounds.first() {
                    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                    if (w - width.get()).abs() >= 0.5 || (h - height.get()).abs() >= 0.5 {
                        width.set(w);
                        height.set(h);
                        cx.notify(shell);
                        // A floated panel is its own window and doesn't
                        // redraw on the shell's notify; ask it directly, once
                        // this draw is over (a refresh mid-draw is dropped).
                        let handle = window.window_handle();
                        cx.defer(move |cx| {
                            let _ = handle.update(cx, |_, window, _| window.refresh());
                        });
                    }
                }
            })
            .child(body);

        (
            Some(overflow.into_any_element()),
            Some(measured.into_any_element()),
        )
    }
}

#[cfg(test)]
mod tests;
