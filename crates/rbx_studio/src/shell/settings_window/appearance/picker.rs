//! The colour popover, for a custom accent and for a tool's colour: a
//! saturation/value square, a hue bar, the hex, and the bars the colour has
//! to clear. What it offers to apply follows from those: the colour itself
//! when everything passes; the nearest lighter colour that passes when a
//! contrast bar fails, never the raw one; the colour anyway, with a warning,
//! when the only trouble is a hue close to a status colour.

use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::accent;
use crate::tokens;

use super::super::kit::{icon, mono};
use super::super::SettingsWindow;

mod parts;
mod state;

use parts::{button, check_row, gradient, measure, note, thumb};
use state::Drag;
pub(in crate::shell::settings_window) use state::{Picker, Target};

const WIDTH: f32 = 280.;
const PADDING: f32 = 14.;
const SQUARE_HEIGHT: f32 = 140.;
/// The popover at its tallest (four check rows and a two-line note), which
/// its position leaves room for up front: a popover that moved to fit as a
/// note appeared would slide under a pointer dragging on it.
const TALLEST: f32 = 480.;

impl SettingsWindow {
    /// Opens the popover for `target`, starting from `start`, its top-left
    /// at `anchor`.
    pub(in crate::shell::settings_window) fn open_picker(
        &mut self,
        target: Target,
        start: Rgba,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bottom = window.viewport_size().height - px(8.);
        let anchor = point(anchor.x, anchor.y.min(bottom - px(TALLEST)).max(px(8.)));
        self.picker = Some(Picker::new(target, start, anchor, window, cx));
        cx.notify();
    }

    fn apply_picked(&mut self, color: Rgba, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        self.shell.update(cx, |shell, cx| match picker.target {
            Target::Accent => shell.set_accent(Some(color), cx),
            Target::Tool(tool) => shell.set_tool_color(tool, Some(color), cx),
        });
        cx.notify();
    }

    pub(in crate::shell::settings_window) fn picker_popover(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let picker = self.picker.as_ref()?;
        let candidate = picker.candidate();
        let pure_hue = accent::from_hsv(picker.hue, 1., 1.);
        let checks = picker.checks();
        let failing = checks.iter().any(|check| !check.passes());
        let status = picker.status();
        let applied = picker.applied();
        let app_accent = tokens::check_on();

        let square = div()
            .relative()
            .h(px(SQUARE_HEIGHT))
            .rounded(px(6.))
            .bg(pure_hue)
            .child(div().absolute().inset_0().rounded(px(6.)).bg(gradient(
                90.,
                rgb(0xFFFFFF),
                Rgba {
                    a: 0.,
                    ..rgb(0xFFFFFF)
                },
            )))
            .child(div().absolute().inset_0().rounded(px(6.)).bg(gradient(
                0.,
                rgb(0x000000),
                Rgba {
                    a: 0.,
                    ..rgb(0x000000)
                },
            )))
            .child(measure(picker.square.clone()))
            .child(
                thumb(candidate)
                    .left(relative(picker.saturation))
                    .top(relative(1. - picker.value))
                    .ml(px(-7.))
                    .mt(px(-7.)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if let Some(picker) = &mut this.picker {
                        picker.drag = Some(Drag::Square);
                        picker.drag_to(event.position, window, cx);
                        cx.notify();
                    }
                }),
            );
        const HUES: [u32; 7] = [
            0xFF0000, 0xFFFF00, 0x00FF00, 0x00FFFF, 0x0000FF, 0xFF00FF, 0xFF0000,
        ];
        let bar = div()
            .relative()
            .h(px(12.))
            .child(
                h_flex()
                    .absolute()
                    .inset_0()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .children(HUES.windows(2).map(|pair| {
                        div()
                            .flex_1()
                            .h_full()
                            .bg(gradient(90., rgb(pair[0]), rgb(pair[1])))
                    })),
            )
            .child(measure(picker.bar.clone()))
            .child(
                thumb(pure_hue)
                    .top(px(-1.))
                    .left(relative(picker.hue))
                    .ml(px(-7.)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if let Some(picker) = &mut this.picker {
                        picker.drag = Some(Drag::Bar);
                        picker.drag_to(event.position, window, cx);
                        cx.notify();
                    }
                }),
            );
        let hex_row = h_flex()
            .gap(px(8.))
            .items_center()
            .child(
                div()
                    .flex_none()
                    .size(px(30.))
                    .rounded(px(6.))
                    .bg(candidate)
                    .border_1()
                    .border_color(tokens::border2()),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .h(px(30.))
                    .gap(px(6.))
                    .px(px(10.))
                    .items_center()
                    .border_1()
                    .border_color(tokens::accent_line())
                    .rounded(px(6.))
                    .bg(tokens::dock())
                    .child(mono(11.5, 16.).text_color(tokens::text3()).child("HEX"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family(tokens::FONT_FAMILY_MONO)
                            .child(
                                Input::new(&picker.hex)
                                    .appearance(false)
                                    .px_0()
                                    .text_size(px(11.5))
                                    .line_height(px(16.))
                                    .text_color(tokens::text()),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .id("pipette")
                    .flex_none()
                    .size(px(30.))
                    .items_center()
                    .justify_center()
                    .border_1()
                    .border_color(tokens::border2())
                    .rounded(px(6.))
                    .bg(tokens::dock())
                    .text_color(tokens::text3())
                    .child(icon("pipette", 14.))
                    .tooltip(|window, cx| {
                        super::super::super::tooltip::text(
                            "Picking a colour from the screen isn\u{2019}t available yet",
                            window,
                            cx,
                        )
                    }),
            );
        let rows = v_flex()
            .gap(px(2.))
            .pt(px(10.))
            .border_t_1()
            .border_color(tokens::border())
            .children(checks.iter().map(check_row))
            .children(status.map(|status| {
                h_flex()
                    .h(px(20.))
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .text_color(tokens::warning())
                            .child(icon("triangle-alert", 11.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.5))
                            .text_color(tokens::text2())
                            .child(format!("Close to the {}", status.name())),
                    )
                    .child(mono(10.5, 14.).text_color(tokens::warning()).child("hue"))
            }));
        let message = if failing {
            applied.map(|fixed| {
                note(tokens::text_error(), "circle-alert", {
                    let lead = "Too dark to read on. ";
                    let hex = accent::hex(fixed);
                    let body = format!("{lead}{hex} keeps the hue and passes every check.");
                    let highlight = HighlightStyle {
                        color: Some(tokens::text().into()),
                        ..Default::default()
                    };
                    StyledText::new(body)
                        .with_highlights([(lead.len()..lead.len() + hex.len(), highlight)])
                })
            })
        } else {
            status.map(|status| {
                note(
                    tokens::warning(),
                    "triangle-alert",
                    format!(
                        "Readable, but selected rows may look like errors next to the {}.",
                        status.name()
                    ),
                )
            })
        };
        let primary = match (failing, status, applied) {
            (true, _, Some(fixed)) => Some(button(
                "picker-apply",
                format!("Use {}", accent::hex(fixed)),
                Some(fixed),
            )),
            (true, _, None) => None,
            (false, Some(_), Some(_)) => {
                Some(button("picker-apply", "Apply anyway", Some(app_accent)))
            }
            (false, None, Some(_)) => Some(button("picker-apply", "Apply", Some(app_accent))),
            (false, _, None) => None,
        }
        .map(|button| {
            button.on_click(cx.listener(move |this, _, _, cx| {
                if let Some(color) = applied {
                    this.apply_picked(color, cx);
                }
            }))
        });
        let buttons = h_flex()
            .justify_end()
            .gap(px(8.))
            .child(
                button("picker-cancel", "Cancel", None).on_click(cx.listener(|this, _, _, cx| {
                    this.picker = None;
                    cx.notify();
                })),
            )
            .children(primary);

        let popover = v_flex()
            .id("picker")
            .occlude()
            .w(px(WIDTH))
            .gap(px(12.))
            .p(px(PADDING))
            .border_1()
            .border_color(tokens::border2())
            .rounded(px(10.))
            .bg(tokens::field_select())
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.55),
                offset: point(px(0.), px(18.)),
                blur_radius: px(48.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                if let Some(picker) = &mut this.picker {
                    if picker.drag.is_some() {
                        picker.drag_to(event.position, window, cx);
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    if let Some(picker) = &mut this.picker {
                        picker.drag = None;
                    }
                }),
            )
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.picker = None;
                cx.notify();
            }))
            .child(square)
            .child(bar)
            .child(hex_row)
            .child(rows)
            .children(message)
            .child(buttons);
        let anchor = picker.anchor;
        Some(deferred(anchored().position(anchor).child(popover)).into_any_element())
    }
}
