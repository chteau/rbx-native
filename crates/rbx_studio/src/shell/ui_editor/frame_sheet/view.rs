//! The sheet's look: a header, a tool strip, the stage and its side panel.
//! Every size here and in `kit`/`panel` is the board's
//! (`KEVIN_0127`–`0134_VPFrame-*`), kept as plain numbers so a restyle
//! is an edit to these three files alone.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::*;

use super::kit::{icon_tool, mono, separator, sized, stage_size, text_button, Look};
use super::{Popover, Shell, Tool};
use crate::tokens;
use crate::viewport_frame as frame;
use crate::workspace_view::FrameRequest;

/// How far the sheet stands in from the canvas dock's edges.
const INSET: f32 = 20.0;
/// The stage's surround: a near-black a step off the window's own, like the
/// canvas's backdrop a fixed colour rather than a theme's, since it frames
/// a picture and not the chrome.
const SURROUND: u32 = 0x0b0b0c;
/// The hatch outside the stage: a hairline every this many pixels.
const HATCH_STEP: f32 = 8.0;

impl Shell {
    /// The sheet over the canvas — the backdrop, then the sheet itself.
    pub(in crate::shell::ui_editor) fn frame_sheet(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let target = self.ui.sheet.as_ref()?.frame;
        self.show_frame_fov(window, cx);
        let size = self.frame_size(target, cx).unwrap_or([1.0, 1.0]);
        let name = self
            .dom
            .get(target)
            .map(|instance| instance.name().to_owned())
            .unwrap_or_default();
        let header = self.sheet_header(name, size, cx);
        let strip = self.sheet_strip(cx);
        let stage = self.sheet_stage(target, size, cx);
        let panel = self.sheet_panel(target, window, cx);
        let sheet = self.ui.sheet.as_ref()?;
        let (focus, popover) = (sheet.focus.clone(), sheet.popover);
        let popover = popover.map(|popover| self.sheet_popover(popover, target, window, cx));

        let body = v_flex()
            .id("frame-sheet")
            .track_focus(&focus)
            .absolute()
            .top(px(INSET))
            .left(px(INSET))
            .right(px(INSET))
            .bottom(px(INSET))
            .bg(tokens::dock())
            .border_1()
            .border_color(tokens::border2())
            .rounded(px(8.))
            // CSS `0 18px 48px`: GPUI's blur reads about twice a browser's.
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.55),
                offset: point(px(0.), px(18.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .overflow_hidden()
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.frame_key(&event.keystroke, true, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_key_up(cx.listener(|shell, event: &KeyUpEvent, window, cx| {
                if shell.frame_key(&event.keystroke, false, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(header)
            .child(strip)
            .child(h_flex().flex_1().min_h_0().child(stage).child(panel))
            .children(popover);

        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .child(div().absolute().inset_0().bg(hsla(0., 0., 0., 0.5)))
                .child(body)
                .into_any_element(),
        )
    }

    fn sheet_header(&mut self, name: String, size: [f32; 2], cx: &mut Context<Self>) -> Div {
        // `pt(1)`: centred in an odd height, a browser rounds the half
        // pixel down the page; this does the same.
        h_flex()
            .h(px(40.))
            .flex_none()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .pt(px(1.))
            .border_b_1()
            .border_color(tokens::border())
            .child(
                text_button(
                    "frame-sheet-back",
                    IconName::ArrowLeft,
                    "Canvas",
                    Look::Ghost,
                )
                .on_click(cx.listener(|shell, _, window, cx| shell.close_frame_sheet(window, cx))),
            )
            .child(
                div()
                    .text_size(tokens::text_action())
                    .line_height(px(16.))
                    .font_weight(tokens::WEIGHT_BOLD)
                    .text_color(tokens::text())
                    .child("Edit viewport"),
            )
            .child(
                h_flex()
                    .h(px(22.))
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .border_1()
                    .border_color(tokens::border2())
                    .rounded(px(4.))
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text2())
                    .child(Icon::new(IconName::Box).size(px(12.)))
                    .child(name),
            )
            .child(div().flex_1())
            .child(mono(sized(size), 11.0, tokens::text2()))
            .child(
                text_button("frame-sheet-done", IconName::Check, "Done", Look::Primary).on_click(
                    cx.listener(|shell, _, window, cx| shell.close_frame_sheet(window, cx)),
                ),
            )
    }

    fn sheet_strip(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(sheet) = self.ui.sheet.as_ref() else {
            return div();
        };
        let (tool, popover, background) = (sheet.tool, sheet.popover, sheet.background);
        let set_tool = |tool: Tool| {
            cx.listener(
                move |shell: &mut Shell, _: &ClickEvent, _: &mut Window, cx| {
                    if let Some(sheet) = &mut shell.ui.sheet {
                        sheet.tool = tool;
                        cx.notify();
                    }
                },
            )
        };
        let insert_look = match popover == Some(Popover::Picker) {
            true => Look::On,
            false => Look::Ghost,
        };
        let fit_look = match popover == Some(Popover::Fit) {
            true => Look::On,
            false => Look::Ghost,
        };
        h_flex()
            .h(px(36.))
            .flex_none()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .border_b_1()
            .border_color(tokens::border())
            .child(
                text_button(
                    "frame-sheet-insert",
                    IconName::PackagePlus,
                    "Insert…",
                    insert_look,
                )
                .on_click(
                    cx.listener(|shell, _, _, cx| shell.toggle_frame_popover(Popover::Picker, cx)),
                ),
            )
            .child(separator())
            .child(
                icon_tool(
                    "frame-sheet-orbit",
                    IconName::Orbit,
                    "Orbit",
                    tool == Tool::Orbit,
                )
                .on_click(set_tool(Tool::Orbit)),
            )
            .child(
                icon_tool(
                    "frame-sheet-hand",
                    IconName::Hand,
                    "Pan",
                    tool == Tool::Hand,
                )
                .on_click(set_tool(Tool::Hand)),
            )
            .child(separator())
            .child(
                text_button("frame-sheet-fit", IconName::Scan, "Fit", fit_look)
                    .child(Icon::new(IconName::ChevronDown).size(px(12.)))
                    .on_click(
                        cx.listener(|shell, _, _, cx| shell.toggle_frame_popover(Popover::Fit, cx)),
                    ),
            )
            .child(
                icon_tool(
                    "frame-sheet-reset",
                    IconName::RotateCcw,
                    "Reset camera (Home)",
                    false,
                )
                .on_click(cx.listener(|shell, _, _, cx| shell.reset_frame_camera(cx))),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .id("frame-sheet-background")
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .text_size(tokens::text_md())
                    .text_color(tokens::text2())
                    .hover(|this| this.text_color(tokens::text()))
                    .child(
                        Icon::new(match background {
                            true => IconName::Eye,
                            false => IconName::EyeOff,
                        })
                        .size(px(14.)),
                    )
                    .child("Frame background")
                    .on_click(cx.listener(|shell, _, _, cx| {
                        if let Some(sheet) = &mut shell.ui.sheet {
                            sheet.background = !sheet.background;
                            cx.notify();
                        }
                    })),
            )
    }

    fn sheet_stage(
        &mut self,
        target: rbx_dom::Ref,
        size: [f32; 2],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let aspect = size[0] / size[1].max(1.0);
        let Some(sheet) = self.ui.sheet.as_ref() else {
            return div().into_any_element();
        };
        let area = sheet.area.clone();
        let room = area.get().size;
        let stage = stage_size(room, aspect);
        // Centred the way a browser centres, a half pixel rounding down
        // and right: flex centring here would put it a pixel higher.
        let corner = [
            ((f32::from(room.width) - stage.0 as f32) / 2.0).round(),
            ((f32::from(room.height) - stage.1 as f32) / 2.0).round(),
        ];
        sheet.request.set(Some(FrameRequest {
            frame: target,
            size: stage,
            background: sheet.background,
            backdrop: [0x0b, 0x0b, 0x0c],
        }));
        let image = self
            .viewport
            .read(cx)
            .canvas()
            .and_then(|canvas| canvas.frame.clone())
            .filter(|(request, _)| request.frame == target)
            .map(|(_, image)| image);
        let empty = frame::contents_of(&self.dom, &self.database, target).is_empty();
        // The picture even when empty: it is the frame's own background.
        let picture = image.map(|image| img(image).size_full());
        let empty = empty.then(|| self.sheet_empty(cx).absolute().inset_0());
        let readout = format!("stage {} × {} · frame {}", stage.0, stage.1, sized(size));
        div()
            .id("frame-sheet-area")
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .bg(rgb(SURROUND))
            .child(hatch(area))
            .child(
                div()
                    .absolute()
                    .left(px(corner[0]))
                    .top(px(corner[1]))
                    .w(px(stage.0 as f32))
                    .h(px(stage.1 as f32))
                    .shadow(vec![BoxShadow {
                        color: tokens::border2().into(),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(1.),
                        inset: false,
                    }])
                    .overflow_hidden()
                    .children(picture)
                    .children(empty),
            )
            .child(
                mono(readout, 10.5, tokens::text3())
                    .absolute()
                    .left(px(12.))
                    .bottom(px(10.)),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Shell::frame_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Shell::frame_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Shell::frame_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Shell::frame_mouse_up))
            // Let go outside the stage, a drag still has to end there.
            .on_mouse_up_out(MouseButton::Left, cx.listener(Shell::frame_mouse_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Shell::frame_mouse_up))
            .on_mouse_move(cx.listener(Shell::frame_mouse_move))
            .on_scroll_wheel(cx.listener(Shell::frame_scroll))
            .into_any_element()
    }
}

/// The surround's hairline hatch, 135°, and where the area was laid out —
/// what the stage is sized against next frame.
fn hatch(area: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(
        move |bounds, _, _| area.set(bounds),
        |bounds, _, window, _| {
            let line = hsla(0., 0., 1., 0.025);
            let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let origin = bounds.origin;
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                // Lines down-left at 45°: x + y is constant along each.
                let mut sum = 0.0;
                while sum < width + height {
                    // `add_polygon`, not `move_to`/`line_to`: a stroke
                    // built from those two paints nothing.
                    let mut builder = PathBuilder::stroke(px(1.));
                    builder.add_polygon(
                        &[
                            origin + point(px(sum), px(0.)),
                            origin + point(px(sum - height), px(height)),
                        ],
                        false,
                    );
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, line);
                    }
                    sum += HATCH_STEP * std::f32::consts::SQRT_2;
                }
            });
        },
    )
    .absolute()
    .inset_0()
}
