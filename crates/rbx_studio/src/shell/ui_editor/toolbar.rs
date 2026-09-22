//! The document's strip: the two sub-tabs as dock-tab pills (the same
//! pill and strip every dock wears — `chrome::tab_pill`/`dock_strip`), and,
//! on the canvas, its tools in the strip's trailing cell: align, distribute
//! and group for the selection, the simulated screen, and the zoom.

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Shell, Tab};
use crate::align::Mode;
use crate::tokens;
use crate::ui_canvas::PRESETS;

use super::super::chrome;
use super::super::menu::{self, MenuId};
use super::super::workspace::search_field;

/// One align button: the axis, the side, and how it reads.
const ALIGNS: [(usize, Mode, IconName, &str); 6] = [
    (0, Mode::Min, IconName::AlignStartVertical, "Align left"),
    (
        0,
        Mode::Center,
        IconName::AlignCenterVertical,
        "Align horizontal centres",
    ),
    (0, Mode::Max, IconName::AlignEndVertical, "Align right"),
    (1, Mode::Min, IconName::AlignStartHorizontal, "Align top"),
    (
        1,
        Mode::Center,
        IconName::AlignCenterHorizontal,
        "Align vertical centres",
    ),
    (1, Mode::Max, IconName::AlignEndHorizontal, "Align bottom"),
];

/// How much one zoom button press scales the canvas.
const ZOOM_STEP: f32 = 1.25;

impl Shell {
    pub(super) fn ui_tabs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.ui.tab;
        self.ui.nav.begin(&self.tab_order, Some(Tab::ALL.len()), cx);
        let tabs: Vec<AnyElement> = Tab::ALL
            .into_iter()
            .enumerate()
            .map(|(index, tab)| {
                let pill = chrome::tab_pill(tab.key(), tab.label().into(), tab == active)
                    .on_click(cx.listener(move |shell, _, _, cx| shell.set_ui_tab(tab, cx)));
                self.ui.nav.item(index, pill, cx).into_any_element()
            })
            .collect();
        let tools = (active == Tab::Canvas).then(|| self.canvas_tools(cx));

        div()
            .w_full()
            .flex_none()
            .bg(tokens::dock())
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.ui.nav.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(chrome::dock_strip(tabs, tools, true))
            .into_any_element()
    }

    fn canvas_tools(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (width, height) = self.ui.resolution;

        let count = self.selected_all().len();
        let aligns = ALIGNS.map(|(axis, mode, icon, label)| {
            tool(
                ("ui-align", axis * 3 + mode as usize),
                icon,
                label,
                count >= 2,
            )
            .on_click(cx.listener(move |shell, _, _, cx| shell.align_gui(axis, mode, cx)))
            .into_any_element()
        });
        let distributes = [
            (
                0,
                IconName::AlignHorizontalDistributeCenter,
                "Distribute horizontally",
            ),
            (
                1,
                IconName::AlignVerticalDistributeCenter,
                "Distribute vertically",
            ),
        ]
        .map(|(axis, icon, label)| {
            tool(("ui-distribute", axis), icon, label, count >= 3)
                .on_click(cx.listener(move |shell, _, _, cx| shell.distribute_gui(axis, cx)))
                .into_any_element()
        });
        let group = tool(
            "ui-group",
            IconName::Group,
            "Group in a frame (Ctrl+G)",
            count >= 1,
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.group_selected(cx)));
        let responsive = chrome::icon_button(
            "ui-responsive",
            IconName::Scaling,
            "Make responsive: offsets to scale, fixed shapes kept by aspect ratio",
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.make_responsive(cx)));

        let presets = PRESETS
            .iter()
            .map(|&(label, w, h)| {
                menu::item(label)
                    .checked((w, h) == (width, height))
                    .on_click(move |shell, cx| shell.set_resolution((w, h), cx))
            })
            .collect();
        let resolution = menu::dropdown(
            self,
            MenuId::UiResolution,
            chrome::Trigger::new(
                chrome::button("ui-resolution", format!("{width}×{height}"), false)
                    .gap(px(4.))
                    .child(gpui_kit::component::Icon::new(IconName::ChevronDown).size(px(10.))),
            ),
            presets,
            cx,
        );

        // A `BillboardGui`/`SurfaceGui` is the size its own properties and
        // its part make it: nothing a device preset could change, so the
        // strip says what that size is instead of offering one.
        let own_size = self
            .ui
            .screen
            .is_some_and(|root| !super::takes_resolution(&self.dom, &self.database, root));
        let screen: Vec<AnyElement> = match own_size {
            true => {
                let (w, h) = self.canvas_size(cx);
                vec![div()
                    .id("ui-own-size")
                    .px(px(8.))
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_label())
                    .tooltip(|window, cx| {
                        super::super::tooltip::text(
                            "A BillboardGui or SurfaceGui is drawn at its own canvas size",
                            window,
                            cx,
                        )
                    })
                    .child(format!("{w}×{h} canvas"))
                    .into_any_element()]
            }
            false => vec![
                resolution.into_any_element(),
                size_field(self.tab_order.next(), &self.ui.width).into_any_element(),
                div()
                    .text_color(tokens::text_muted())
                    .child("×")
                    .into_any_element(),
                size_field(self.tab_order.next(), &self.ui.height).into_any_element(),
                chrome::icon_button(
                    "ui-orientation",
                    IconName::RotateCw,
                    "Turn the screen: portrait ⇄ landscape",
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.set_resolution((height, width), cx);
                }))
                .into_any_element(),
            ],
        };

        let zoom = format!("{:.0}%", self.ui.view.zoom * 100.0);
        h_flex()
            .flex_1()
            .items_center()
            .gap(px(2.))
            .children(aligns)
            .child(separator())
            .children(distributes)
            .child(separator())
            .child(group)
            .child(responsive)
            .child(div().flex_1())
            .children(screen)
            .child(separator())
            .child(
                chrome::icon_button("ui-zoom-out", IconName::ZoomOut, "Zoom out").on_click(
                    cx.listener(|shell, _, _, cx| shell.zoom_canvas(1.0 / ZOOM_STEP, cx)),
                ),
            )
            .child(
                div()
                    .min_w(px(36.))
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_label())
                    .child(zoom),
            )
            .child(
                chrome::icon_button("ui-zoom-in", IconName::ZoomIn, "Zoom in")
                    .on_click(cx.listener(|shell, _, _, cx| shell.zoom_canvas(ZOOM_STEP, cx))),
            )
            .child(
                chrome::icon_button("ui-fit", IconName::Scan, "Fit the screen to the panel")
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.ui.fitted = true;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Keeps the two size fields showing the resolution, unless one is
    /// being typed in — called while rendering, the only time there is a
    /// `Window` to write a field with, which a menu pick does not have.
    pub(super) fn sync_size_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (width, height) = self.ui.resolution;
        for (input, value) in [
            (self.ui.width.clone(), width),
            (self.ui.height.clone(), height),
        ] {
            let text = value.to_string();
            let stale = {
                let state = input.read(cx);
                state.value() != text.as_str() && !state.focus_handle(cx).is_focused(window)
            };
            if stale {
                input.update(cx, |state, cx| state.set_value(text, window, cx));
            }
        }
    }

    /// Zooms about the middle of the panel, from the toolbar's buttons.
    fn zoom_canvas(&mut self, factor: f32, cx: &mut Context<Self>) {
        let size = self.ui.bounds.get().size;
        let middle = [f32::from(size.width) * 0.5, f32::from(size.height) * 0.5];
        self.ui.view = self.ui.view.zoomed(factor, middle);
        self.ui.fitted = false;
        cx.notify();
    }
}

/// A toolbar icon button, dimmed and inert while there is too little
/// selected for it to do anything.
fn tool(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    enabled: bool,
) -> Stateful<Div> {
    chrome::icon_button(id, icon, label)
        .when(!enabled, |this| this.opacity(0.4).cursor_not_allowed())
}

fn separator() -> impl IntoElement {
    div()
        .flex_none()
        .w(px(1.))
        .h(px(14.))
        .mx(px(4.))
        .bg(tokens::divider())
}

/// One of the width/height fields: the dock search field's own look, sized
/// for four digits and the field's own padding.
fn size_field(
    tab_index: isize,
    state: &Entity<gpui_kit::component::input::InputState>,
) -> impl IntoElement {
    div()
        .w(px(72.))
        .flex_none()
        .child(search_field(tab_index, state))
}
