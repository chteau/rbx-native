//! The Viewport dock: the graphics quality, every view setting, and the live
//! frame rate — kept in a dock so that nothing persistent sits over the scene
//! being edited.
//!
//! The settings are the shell's own state and apply whether or not this dock
//! is open; the dock only shows them. The frame-rate sampling is the other
//! way round: it runs only while the dock is on screen (see
//! [`Shell::sync_stats`]), because a readout nobody can see is per-frame
//! work on both the render and the UI thread for nothing.

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::select::Select;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::pacing::UnfocusedFps;
use crate::settings::DraggerSettings;
use crate::tokens;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::rows::{self, checkbox};
use super::Shell;

/// The quality dropdown's width: its longest label ("Automatic") at the UI
/// scale, and the chevron's own room, which does not scale.
fn quality_width() -> Pixels {
    tokens::scaled_width(101.) + tokens::select_chevron_room()
}

/// One view setting: what its row reads, whether it is on, and what sets it.
type Toggle = (
    &'static str,
    bool,
    fn(&mut Shell, bool, &mut Context<Shell>),
);

impl Shell {
    /// Every view setting the dock lists, in the order it lists them. A new
    /// setting is one more entry here and nothing else.
    fn viewport_toggles(&self) -> Vec<Toggle> {
        vec![
            ("Orthographic", self.orthographic, Shell::set_orthographic),
            (
                "Orientation Indicator",
                self.axis_indicator,
                Shell::set_axis_indicator,
            ),
            (
                "Hide Selection Box Behind Parts",
                self.selection_occluded,
                Shell::set_selection_occluded,
            ),
            (
                "Show Light Guides",
                self.light_guides_shown(),
                // A flip, and the row only ever asks for the opposite of
                // what it shows, so the value it passes is already implied.
                |shell, _, cx| shell.toggle_light_guides(cx),
            ),
            // Studio's dragger settings, under Studio's own names (see
            // `settings::DraggerSettings`).
            (
                "Show Hover Ruler",
                self.dragger().show_hover_ruler,
                |shell, show_hover_ruler, cx| {
                    shell.set_dragger(
                        DraggerSettings {
                            show_hover_ruler,
                            ..shell.dragger()
                        },
                        cx,
                    )
                },
            ),
            (
                "Show Target Snap",
                self.dragger().show_target_snap,
                |shell, show_target_snap, cx| {
                    shell.set_dragger(
                        DraggerSettings {
                            show_target_snap,
                            ..shell.dragger()
                        },
                        cx,
                    )
                },
            ),
            (
                "Show Dragged Point",
                self.dragger().show_dragged_point,
                |shell, show_dragged_point, cx| {
                    shell.set_dragger(
                        DraggerSettings {
                            show_dragged_point,
                            ..shell.dragger()
                        },
                        cx,
                    )
                },
            ),
            (
                "Show Measurement",
                self.dragger().show_measurement,
                |shell, show_measurement, cx| {
                    shell.set_dragger(
                        DraggerSettings {
                            show_measurement,
                            ..shell.dragger()
                        },
                        cx,
                    )
                },
            ),
            (
                "Snap to Parts",
                self.dragger().snap_to_parts,
                |shell, snap_to_parts, cx| {
                    shell.set_dragger(
                        DraggerSettings {
                            snap_to_parts,
                            ..shell.dragger()
                        },
                        cx,
                    )
                },
            ),
            (
                "Cap frame rate at 25 fps when unfocused",
                self.unfocused_fps == UnfocusedFps::Fps25,
                Shell::set_unfocused_cap,
            ),
        ]
    }

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

    /// The graphics-quality dropdown, in the same box every other field in
    /// the editor wears (`rows::select_box`) rather than the toolkit's own.
    ///
    /// A Tab stop of its own: `SelectState` is `Focusable`, and its handle
    /// is the one `Select` itself focuses, so recording that handle in the
    /// window's order is all it takes. The toolkit draws no focus ring on a
    /// select without its own chrome, so the box draws the editor's —
    /// `focus_visible`'s rule by hand, since the handle is not this
    /// element's: focused, and reached by keyboard. Inset, because the
    /// field's column clips anything drawn outside it.
    fn quality_control(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = self.quality.read(cx).focus_handle(cx);
        self.tab_order.register(&handle);
        let ringed = handle.contains_focused(window, cx) && window.last_input_was_keyboard();
        rows::select_box()
            .w(quality_width())
            // A side dock at a large UI scale can be narrower than that;
            // the label truncates rather than the chevron being cut off.
            .max_w_full()
            .when(ringed, |this| this.shadow(tokens::focus_ring_inset()))
            .child(
                Select::new(&self.quality)
                    .appearance(false)
                    .with_size(tokens::field_size())
                    .h_full()
                    .py_0()
                    .pt(tokens::select_inset())
                    .menu_width(quality_width())
                    .accessibility_label("Graphics quality"),
            )
    }

    /// The dock's trailing menu and its body: the quality and the live
    /// numbers as one column, the settings beside it — or under it, once
    /// the dock is too narrow for both.
    ///
    /// The quality select is one Tab stop and the settings are one more:
    /// a roving group, the way the Properties panel's checkboxes are, so
    /// arrows walk the list in reading order and Tab leaves it — not ten
    /// stops to press through on the way to the next dock.
    pub(super) fn viewport_dock(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::ViewportOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "viewport-overflow",
                IconName::Ellipsis,
                "Viewport dock settings",
            )),
            self.move_items(Panel::Viewport),
            cx,
        );

        // A dock on the bottom edge is wide and short, one on a side the
        // other way round, so both groups are fixed-width cells in a
        // wrapping row: side by side on the bottom, stacked on a side.
        let column = tokens::scaled_width(300.);
        let readout = self.viewport.read(cx).readout();
        let numbers = v_flex()
            .flex_none()
            .w(column)
            .max_w_full()
            .child(field("Graphics quality", self.quality_control(window, cx)))
            .children(readout.map(|(name, value)| {
                field(
                    name,
                    div()
                        .truncate()
                        .text_color(tokens::text_strong())
                        .child(value),
                )
            }));

        let handle = cx.entity();
        let toggles = self.viewport_toggles();
        self.viewport_nav
            .begin(&self.tab_order, Some(toggles.len()), cx);
        let settings: Vec<_> = toggles
            .into_iter()
            .enumerate()
            .map(|(index, (label, on, set))| {
                let handle = handle.clone();
                // The whole row is the target, not only the box: the label
                // is the obvious thing to click. Space and Enter on the
                // focused row arrive as this same click.
                let row = checkbox(
                    SharedString::from(format!("viewport-{label}")),
                    on,
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            set(shell, !on, cx);
                            cx.notify();
                        });
                    },
                )
                .w(column)
                .max_w_full()
                .gap(tokens::label_gap())
                // Inset rather than the checkbox's own outset ring: the
                // rows sit flush, so the next one would paint over the
                // bottom of it. The inset clears the box by this much.
                .pl(tokens::label_gap())
                .rounded(tokens::RADIUS)
                .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_color(tokens::text_label())
                        .child(label),
                );
                self.viewport_nav.item(index, row, cx)
            })
            .collect();

        let body = div()
            .id("viewport-dock")
            .size_full()
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.viewport_nav.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .overflow_y_scroll()
            .track_scroll(&self.viewport_scroll)
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_start()
                    .gap_x(tokens::group_gap())
                    .gap_y(tokens::row_gap())
                    .px(tokens::row_padding())
                    .text_size(tokens::text_md())
                    .line_height(tokens::line_md())
                    .child(numbers)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .flex_grow_1()
                            .flex_basis(column)
                            .gap_x(tokens::group_gap())
                            .children(settings),
                    ),
            )
            .vertical_scrollbar(&self.viewport_scroll);

        (
            Some(overflow.into_any_element()),
            Some(chrome::dock_content(body).into_any_element()),
        )
    }
}

/// A name and its value on one row, the name in the Properties panel's own
/// column width so the two docks read alike.
fn field(name: &'static str, value: impl IntoElement) -> Div {
    h_flex()
        .w_full()
        .min_h(tokens::row_height())
        .items_center()
        .child(
            div()
                .flex_none()
                .w(tokens::row_label_width())
                .pr(tokens::label_gap())
                .truncate()
                .text_color(tokens::text_muted())
                .child(name),
        )
        .child(div().flex_1().overflow_hidden().child(value))
}
