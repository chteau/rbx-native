//! **Row D** — the persistent three-column shell: Properties, the open
//! document with Output docked beneath it, and Explorer.
//!
//! The three panels are plain children of this row and are built once, in
//! `Shell::new`. Nothing here is conditional on which document Row A has
//! open or which page Row B has selected, which is what makes the
//! independence rule hold by construction: switching either tab cannot
//! unmount a panel, so a scroll position or a selection has nowhere to get
//! lost (see `UX_GUIDELINES.md` §7).

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::pacing::UnfocusedFps;
use crate::tokens::{self, Cast};

use super::chrome::{self, Document, Drag, Handle};
use super::menu::{self, MenuId};
use super::Shell;

pub(super) const EXPLORER_WIDTH: f32 = 260.;
pub(super) const PROPERTIES_WIDTH: f32 = 280.;
pub(super) const OUTPUT_HEIGHT: f32 = 160.;
/// A column may not be dragged outside this range; past either end the
/// panel stops being usable rather than merely small.
const COLUMN_RANGE: (f32, f32) = (200., 420.);
const OUTPUT_RANGE: (f32, f32) = (80., 400.);
/// A collapsed Output dock is exactly its own header — enough to find and
/// re-open, and nothing else.
const OUTPUT_COLLAPSED_HEIGHT: f32 = 32.;

impl Shell {
    pub(super) fn workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + 'static {
        let properties = self.properties_panel(window, cx);
        let document = self.document_content(window, cx);
        let output = self.output_dock(cx);
        let explorer = self.explorer_panel(cx);

        h_flex()
            .w_full()
            .flex_1()
            .overflow_hidden()
            .bg(tokens::bg_0())
            .child(
                div()
                    .flex_none()
                    .w(px(self.properties_width))
                    .h_full()
                    .child(chrome::panel(Cast::Right, tokens::RADIUS_LG, properties)),
            )
            .child(self.handle(Handle::Properties, cx))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .bg(tokens::bg_viewport())
                            .child(document),
                    )
                    .when(!self.output_collapsed, |this| {
                        this.child(self.handle(Handle::Output, cx))
                    })
                    .child(
                        div()
                            .flex_none()
                            .w_full()
                            .h(px(self.output_dock_height()))
                            .child(chrome::panel(Cast::Up, tokens::RADIUS_MD, output)),
                    ),
            )
            .child(self.handle(Handle::Explorer, cx))
            .child(
                div()
                    .flex_none()
                    .w(px(self.explorer_width))
                    .h_full()
                    .child(chrome::panel(Cast::Left, tokens::RADIUS_LG, explorer)),
            )
    }

    fn output_dock_height(&self) -> f32 {
        if self.output_collapsed {
            OUTPUT_COLLAPSED_HEIGHT
        } else {
            self.output_height
        }
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

    fn explorer_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let show_all = self.show_all_services();
        let light_icons = self.icon_pack() == IconPack::Light;
        let overflow = menu::dropdown(
            self,
            MenuId::ExplorerOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "explorer-overflow",
                "more",
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
            .child(chrome::panel_header("Explorer".into(), overflow))
            .child(div().flex_1().overflow_hidden().child(self.explorer(cx)))
            .into_any_element()
    }

    fn properties_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let title = self.properties_title();
        let rows = self.properties(window, cx);

        v_flex()
            .size_full()
            .child(chrome::panel_header(title, div()))
            .child(div().flex_1().overflow_hidden().child(rows))
            .into_any_element()
    }

    /// Output keeps its filter/Clear strip and its own settings menu, and
    /// gains the collapse toggle §4.1 asks for: collapsed, the dock is its
    /// header and nothing else.
    fn output_dock(&self, cx: &mut Context<Self>) -> AnyElement {
        let timestamps = self.output_show_timestamps;
        let collapsed = self.output_collapsed;
        let overflow = menu::dropdown(
            self,
            MenuId::OutputOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "output-overflow",
                "more",
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
            .items_center()
            .gap(tokens::SPACE_1)
            .child(self.output_controls(cx))
            .child(overflow)
            .child(
                chrome::icon_button(
                    "output-collapse",
                    if collapsed {
                        "chevron-up"
                    } else {
                        "chevron-down"
                    },
                    if collapsed {
                        "Expand Output"
                    } else {
                        "Collapse Output"
                    },
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.output_collapsed = !shell.output_collapsed;
                    cx.notify();
                })),
            );

        v_flex()
            .size_full()
            .child(chrome::panel_header("Output".into(), controls))
            .when(!collapsed, |this| {
                this.child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(self.output_panel(cx)),
                )
            })
            .into_any_element()
    }

    /// The viewport's own settings, which used to hang off the dock's
    /// Viewport tab. They belong to the document, so Row A carries them
    /// (see `chrome::document_tabs`).
    pub(super) fn viewport_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + 'static {
        let orthographic = self.orthographic();
        let axis_indicator = self.axis_indicator();
        let stats = self.stats_shown();
        let capped = self.unfocused_fps() == UnfocusedFps::Fps25;

        menu::dropdown(
            self,
            MenuId::ViewportOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "viewport-overflow",
                "more",
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
        )
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

    pub(super) fn end_resize(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }
}
