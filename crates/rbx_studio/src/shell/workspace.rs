//! **Row D** — the persistent three-column shell: Properties, the open
//! document with Output docked beneath it, and Explorer.
//!
//! The three panels are plain children of this row and are built once, in
//! `Shell::new`. Nothing here is conditional on which document Row A has
//! open or which page Row B has selected, which is what makes the
//! independence rule hold by construction: switching either tab cannot
//! unmount a panel, so a scroll position or a selection has nowhere to get
//! lost (see `UX_GUIDELINES.md`).
//!
//! The frame fixes both side docks at 228px. They are draggable here
//! anyway — a place file's instance names are not 228px wide just because
//! the design's were — but they start there, and the handle that widens
//! them draws nothing until it is pointed at.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::pacing::UnfocusedFps;
use crate::tokens;

use super::chrome::{self, Document, Drag, Handle};
use super::menu::{self, MenuId};
use super::Shell;

/// What a dock starts at. The frame fixes both at 228px against a 9px
/// label; this shell sets text at 14px, so the label column — and with it
/// the dock — is wider (see `tokens::dock_width`).
pub(super) fn explorer_width() -> f32 {
    tokens::dock_width()
}

pub(super) fn properties_width() -> f32 {
    tokens::dock_width()
}

pub(super) const OUTPUT_HEIGHT: f32 = 180.;
/// A column may not be dragged outside this range; past either end the
/// panel stops being usable rather than merely small.
const COLUMN_RANGE: (f32, f32) = (200., 560.);
const OUTPUT_RANGE: (f32, f32) = (80., 400.);
/// A persisted dock size, or the default when nothing was saved. Zero is
/// the "nothing was saved" marker (see `Settings`), which also rejects the
/// NaN and negative values a hand-edited file could otherwise inject.
pub(super) fn saved_or_default(saved: f32, default: f32) -> f32 {
    if saved > 0. {
        saved
    } else {
        default
    }
}

/// The most of the window's width one side dock may occupy.
const MAX_DOCK_SHARE: f32 = 0.28;

impl Shell {
    pub(super) fn workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + 'static {
        // At 2x the UI scale a 300px dock becomes 600, and two of them
        // leave a 1600px window ~350px of viewport. The scale is meant to
        // make the editor readable, not to squeeze out the thing being
        // edited, so a dock may never take more than this share of the
        // window however big its own tokens have grown.
        let limit = f32::from(window.viewport_size().width) * MAX_DOCK_SHARE;
        self.properties_width = self.properties_width.min(limit);
        self.explorer_width = self.explorer_width.min(limit);

        let properties = self.properties_dock(window, cx);
        let document = self.document_content(window, cx);
        let output = self.output_dock(cx);
        let explorer = self.explorer_dock(cx);
        let overlay = self.viewport_overlay(cx);
        let showing_viewport = self.document == Document::Viewport;

        h_flex()
            .w_full()
            .flex_1()
            .overflow_hidden()
            .bg(tokens::black())
            .child(
                dock_column()
                    .w(px(self.properties_width))
                    .border_r(px(1.))
                    .border_color(tokens::divider())
                    .child(properties),
            )
            .child(self.handle(Handle::Properties, cx))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        v_flex()
                            .relative()
                            .flex_1()
                            .overflow_hidden()
                            .child(document)
                            // The viewport's own settings have no home in
                            // the frame's chrome — they belong to the open
                            // document, not to a dock — so they float in its
                            // corner, the way every 3D editor's view
                            // controls do. Top *left*: the orientation
                            // indicator already owns the other one.
                            .when(showing_viewport, |this| this.child(overlay)),
                    )
                    .when(!self.output_collapsed, |this| {
                        this.child(self.handle(Handle::Output, cx))
                    })
                    .child(
                        v_flex()
                            .flex_none()
                            .w_full()
                            .bg(tokens::dock())
                            .border_t(px(1.))
                            .border_color(tokens::divider())
                            .when(!self.output_collapsed, |this| {
                                this.h(px(self.output_height))
                            })
                            .child(output),
                    ),
            )
            .child(self.handle(Handle::Explorer, cx))
            .child(
                dock_column()
                    .w(px(self.explorer_width))
                    .border_l(px(1.))
                    .border_color(tokens::divider())
                    .child(explorer),
            )
    }

    /// Whichever editor Row A has open. This is the *only* thing Row A
    /// switches — see this module's own doc comment.
    fn document_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.document {
            Document::Viewport => self.viewport().into_any_element(),
            Document::Scripts => self.script_editor(window, cx).into_any_element(),
            Document::StyleEditor => self.style_editor(window, cx).into_any_element(),
        }
    }

    fn explorer_dock(&self, cx: &mut Context<Self>) -> AnyElement {
        let show_all = self.show_all_services();
        let light_icons = self.icon_pack() == IconPack::Light;
        let overflow = menu::dropdown(
            self,
            MenuId::ExplorerOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "explorer-overflow",
                IconName::Ellipsis,
                "Explorer settings",
            )),
            vec![
                menu::item("Show all services")
                    .checked(show_all)
                    .on_click(move |shell, cx| shell.set_show_all_services(!show_all, cx)),
                menu::item("Light Icons")
                    .checked(light_icons)
                    .on_click(move |shell, cx| {
                        let next = if light_icons {
                            IconPack::Dark
                        } else {
                            IconPack::Light
                        };
                        shell.set_icon_pack(next, cx);
                    }),
            ],
            cx,
        );

        v_flex()
            .size_full()
            .overflow_hidden()
            .child(chrome::dock_tabs(
                "Explorer".into(),
                Some(overflow.into_any_element()),
            ))
            .child(chrome::dock_content(
                v_flex()
                    .size_full()
                    .gap(px(10.))
                    .child(search_field(self.tab_order.next(), &self.search))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .child(self.instance_tree(cx)),
                    ),
            ))
            .into_any_element()
    }

    fn properties_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let title = self.properties_title();
        let rows = self.properties(window, cx);

        v_flex()
            .size_full()
            .overflow_hidden()
            .child(chrome::dock_tabs(title, None))
            .child(chrome::dock_content(rows))
            .into_any_element()
    }

    /// Output keeps its filter/Clear strip and its own settings menu, and
    /// collapses to exactly its own tab strip — enough to find and re-open,
    /// and nothing else.
    fn output_dock(&self, cx: &mut Context<Self>) -> AnyElement {
        let timestamps = self.output_show_timestamps;
        let collapsed = self.output_collapsed;
        let overflow = menu::dropdown(
            self,
            MenuId::OutputOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "output-overflow",
                IconName::Ellipsis,
                "Output settings",
            )),
            vec![menu::item("Show Timestamp")
                .checked(timestamps)
                .on_click(move |shell, cx| {
                    shell.output_show_timestamps = !timestamps;
                    cx.notify();
                })],
            cx,
        );

        let controls = h_flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .child(self.output_controls(cx))
            .child(overflow)
            .child(
                chrome::icon_button(
                    "output-collapse",
                    if collapsed {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    },
                    if collapsed {
                        "Expand Output"
                    } else {
                        "Collapse Output"
                    },
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.output_collapsed = !shell.output_collapsed;
                    shell.save_settings();
                    cx.notify();
                })),
            );

        v_flex()
            .size_full()
            .overflow_hidden()
            .child(chrome::dock_tabs(
                "Output".into(),
                Some(controls.into_any_element()),
            ))
            .when(!collapsed, |this| {
                this.child(chrome::dock_content(
                    div()
                        .size_full()
                        .overflow_hidden()
                        .child(self.output_panel(cx)),
                ))
            })
            .into_any_element()
    }

    /// The viewport's own settings, floating in its top-right corner.
    fn viewport_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let orthographic = self.orthographic();
        let axis_indicator = self.axis_indicator();
        let stats = self.stats_shown();
        let capped = self.unfocused_fps() == UnfocusedFps::Fps25;

        let overflow = menu::dropdown(
            self,
            MenuId::ViewportOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "viewport-overflow",
                IconName::Ellipsis,
                "Viewport settings",
            )),
            vec![
                menu::item("Orthographic")
                    .checked(orthographic)
                    .on_click(move |shell, cx| shell.set_orthographic(!orthographic, cx)),
                menu::item("Orientation Indicator")
                    .checked(axis_indicator)
                    .on_click(move |shell, cx| shell.set_axis_indicator(!axis_indicator, cx)),
                menu::item("Stats")
                    .checked(stats)
                    .on_click(move |shell, cx| shell.set_stats_shown(!stats, cx)),
                menu::item("Cap frame rate at 25 fps when unfocused")
                    .checked(capped)
                    .on_click(move |shell, cx| {
                        let next = if capped {
                            UnfocusedFps::Fps30
                        } else {
                            UnfocusedFps::Fps25
                        };
                        shell.set_unfocused_fps(next, cx);
                    }),
            ],
            cx,
        );

        h_flex()
            .absolute()
            .top(px(8.))
            .left(px(8.))
            .items_center()
            .gap(px(4.))
            .p(px(4.))
            .rounded(tokens::RADIUS)
            .bg(tokens::chrome())
            .shadow(tokens::elevation())
            .child(self.quality_control())
            .child(overflow)
            .into_any_element()
    }

    fn handle(&self, handle: Handle, cx: &mut Context<Self>) -> AnyElement {
        let size = match handle {
            Handle::Properties => self.properties_width,
            Handle::Explorer => self.explorer_width,
            Handle::Output => self.output_height,
        };

        chrome::resize_handle(
            handle,
            cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                let origin = match handle {
                    Handle::Output => event.position.y,
                    _ => event.position.x,
                };
                shell.drag = Some(Drag {
                    handle,
                    origin,
                    size: px(size),
                });
                cx.notify();
            }),
        )
        .into_any_element()
    }

    /// Applies an in-progress drag. Sizes follow the pointer exactly — the
    /// delta is measured from where the drag started, not from the previous
    /// frame, so a drag that leaves the window and comes back doesn't
    /// accumulate error.
    pub(super) fn drag_resize(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else {
            return;
        };

        let clamp = |value: f32, (low, high): (f32, f32)| value.clamp(low, high);
        let size = f32::from(drag.size);
        match drag.handle {
            // Properties sits left of its handle, so rightward drag grows it.
            Handle::Properties => {
                self.properties_width =
                    clamp(size + f32::from(position.x - drag.origin), COLUMN_RANGE);
            }
            // Explorer sits right of its handle: rightward drag shrinks it.
            Handle::Explorer => {
                self.explorer_width =
                    clamp(size - f32::from(position.x - drag.origin), COLUMN_RANGE);
            }
            // Output sits below its handle: downward drag shrinks it.
            Handle::Output => {
                self.output_height =
                    clamp(size - f32::from(position.y - drag.origin), OUTPUT_RANGE);
            }
        }
        cx.notify();
    }

    /// Saved on *release*, not on every frame of the drag: a resize is one
    /// decision, and writing the settings file sixty times a second to
    /// record its intermediate states would be absurd.
    pub(super) fn end_resize(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            self.save_settings();
            cx.notify();
        }
    }
}

/// One side dock's column: its own surface, a step off the window's black
/// ground, plus the hairline that says where it ends.
///
/// The frame paints docks the same black as everything behind them, which
/// makes three docks and the window one undifferentiated field — you cannot
/// see where the Explorer stops and the viewport starts. This is the
/// deliberate departure from it.
fn dock_column() -> Div {
    v_flex().flex_none().h_full().bg(tokens::dock())
}

/// A dock's search field: the frame's own, which is a field with a centred
/// placeholder and no border at all — the surface change is the affordance.
///
/// The toolkit `Input` keeps the caret, selection and IME handling; its own
/// chrome is switched off so this container can be the frame's.
pub(super) fn search_field(
    tab_index: isize,
    state: &Entity<gpui_kit::component::input::InputState>,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .h(tokens::input_height())
        .flex_none()
        .items_center()
        .justify_center()
        .px(px(8.))
        .rounded(tokens::RADIUS)
        .bg(tokens::chrome())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(tokens::text_strong())
        .child(
            Input::new(state)
                .appearance(false)
                .with_size(tokens::field_size())
                .h(tokens::input_height())
                // Without this the field sits at the toolkit's default
                // index 0 and sorts ahead of every region — a search box
                // reached before the menu bar.
                .tab_index(tab_index),
        )
}
