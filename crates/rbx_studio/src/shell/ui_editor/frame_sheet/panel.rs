//! The sheet's side panel — Contents, Lighting, Camera, Image — and the
//! stage's empty state.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit::*;

use super::super::inspector::{caption, line, section};
use super::kit::{icon_tool, mono, text_button, Look};
use super::{FrameSheet, Popover, Shell};
use crate::shell::rows::field_box;
use crate::tokens;
use crate::viewport_frame as frame;

const PANEL_WIDTH: f32 = 280.0;

impl Shell {
    pub(super) fn sheet_empty(&mut self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .pt(px(1.))
            .gap(px(10.))
            .child(
                Icon::new(IconName::PackagePlus)
                    .size(px(28.))
                    .text_color(tokens::text3()),
            )
            .child(
                div()
                    .text_size(tokens::text_lg())
                    .line_height(px(18.))
                    .font_weight(tokens::WEIGHT_SEMIBOLD)
                    .text_color(tokens::text())
                    .child("Nothing in this viewport yet"),
            )
            .child(
                div()
                    .max_w(px(300.))
                    .text_center()
                    .text_size(tokens::text_md())
                    .line_height(px(18.))
                    .text_color(tokens::text2())
                    .child(
                        "Add models or parts from the Workspace. They are copied into \
                         the ViewportFrame; the Workspace is not touched.",
                    ),
            )
            .child(
                text_button(
                    "frame-sheet-insert-empty",
                    IconName::PackagePlus,
                    "Insert from Workspace…",
                    Look::Primary,
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(
                    cx.listener(|shell, _, _, cx| shell.toggle_frame_popover(Popover::Picker, cx)),
                ),
            )
    }

    pub(super) fn sheet_panel(
        &mut self,
        target: rbx_dom::Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let rows = frame::contents_of(&self.dom, &self.database, target)
            .into_iter()
            .enumerate()
            .map(|(index, (child, name, class, model))| {
                h_flex()
                    .w_full()
                    .h(px(26.))
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        Icon::new(match model {
                            true => IconName::Package,
                            false => IconName::Box,
                        })
                        .size(px(14.))
                        .text_color(tokens::text2()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(tokens::text_md())
                            .line_height(px(16.))
                            .text_color(tokens::text())
                            .child(name),
                    )
                    .child(mono(class, 10.5, tokens::text3()))
                    .child(
                        icon_tool(
                            ("frame-sheet-remove", index),
                            IconName::Minus,
                            "Remove from the viewport",
                            false,
                        )
                        .on_click(
                            cx.listener(move |shell, _, _, cx| shell.remove_from_frame(child, cx)),
                        ),
                    )
                    .into_any_element()
            })
            .collect();
        let insert = icon_tool(
            "frame-sheet-add",
            IconName::Plus,
            "Insert from Workspace…",
            false,
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.toggle_frame_popover(Popover::Picker, cx)))
        .into_any_element();
        let lighting = self.frame_lighting(window, cx);
        let image = self.frame_image(window, cx);
        let reset = icon_tool(
            "frame-sheet-camera-reset",
            IconName::RotateCcw,
            "Reset the camera",
            false,
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.reset_frame_camera(cx)))
        .into_any_element();
        let fov = self
            .ui
            .sheet
            .as_ref()
            .map(|sheet: &FrameSheet| sheet.fov.clone());
        let camera_rows = fov
            .map(|fov| {
                line(vec![
                    caption("FOV"),
                    field_box()
                        .flex_1()
                        .min_w_0()
                        .gap(px(4.))
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&fov)
                                    .appearance(false)
                                    .with_size(tokens::field_size())
                                    .h_full(),
                            ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(tokens::text_md())
                                .text_color(tokens::text2())
                                .child("°"),
                        )
                        .into_any_element(),
                ])
            })
            .into_iter()
            .collect();
        v_flex()
            .w(px(PANEL_WIDTH))
            .flex_none()
            .h_full()
            .p(px(5.))
            .border_l_1()
            .border_color(tokens::border())
            .bg(tokens::dock())
            .child(section("Contents", vec![insert], rows))
            .child(lighting)
            .child(section("Camera", vec![reset], camera_rows))
            .child(image)
    }
}
