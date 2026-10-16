//! The Network tile and its "Network simulation" popover: Preset, then
//! Inbound and Outbound with Latency, Jitter and Packet loss each, then
//! Reset (Delete for a saved preset), Save and Apply — Studio's layout.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Choice, Field, Way};
use crate::launcher::ui::{self, Weight};
use crate::shell::ribbon::{tile, unavailable_tile};
use crate::shell::Shell;
use crate::tokens;

const WIDTH: f32 = 340.;
const ROWS: [(Field, &str, &str); 3] = [
    (Field::Latency, "Latency", "ms"),
    (Field::Jitter, "Jitter", "ms"),
    (Field::Loss, "Packet loss", "%"),
];

/// The open popover's widgets; the values themselves live in `staged`.
pub(in crate::shell) struct Popover {
    pub(super) inputs: Vec<(Way, Field, Entity<InputState>)>,
    pub(super) menu_open: bool,
    /// The Save row's name field, while it shows.
    pub(super) saving: Option<Entity<InputState>>,
    pub(super) save_error: Option<&'static str>,
    /// The last Apply's outcome when it needs saying.
    pub(in crate::shell) status: Option<String>,
    pub(super) _subscriptions: Vec<Subscription>,
}

fn format(field: Field, value: f64) -> String {
    match field {
        Field::Loss => format!("{value:.2}"),
        _ => {
            let text = format!("{value:.1}");
            text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
        }
    }
}

impl Shell {
    fn network_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.play.network.shown().conditions;
        let mut inputs = Vec::new();
        let mut subscriptions = Vec::new();
        for way in [Way::Inbound, Way::Outbound] {
            let values = match way {
                Way::Inbound => shown.inbound,
                Way::Outbound => shown.outbound,
            };
            for (field, ..) in ROWS {
                let text = format(field, field.get(values));
                let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |shell, input, event: &InputEvent, window, cx| match event {
                        InputEvent::Change => {
                            let typed = input.read(cx).value().trim().to_owned();
                            if let Ok(value) = typed.parse::<f64>() {
                                if shell.play.network.edit(way, field, value) {
                                    cx.notify();
                                }
                            }
                        }
                        InputEvent::Blur | InputEvent::PressEnter { .. } => {
                            shell.network_sync_inputs(window, cx);
                        }
                        _ => {}
                    },
                ));
                inputs.push((way, field, input));
            }
        }
        self.play.network.popover = Some(Popover {
            inputs,
            menu_open: false,
            saving: None,
            save_error: None,
            status: None,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// Studio's dismissal: staged edits are dropped, the pill shows the
    /// applied setting again.
    fn network_close(&mut self, cx: &mut Context<Self>) {
        if self.play.network.popover.take().is_some() {
            self.play.network.closed_at = Some(std::time::Instant::now());
        }
        self.play.network.staged = None;
        cx.notify();
    }

    /// Re-shows the staged values (after a preset pick, Reset, or a typed
    /// value that was clamped). An unchanged value is not an edit, so this
    /// does not turn the preset into Custom.
    pub(super) fn network_sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.play.network.shown().conditions;
        let Some(popover) = &self.play.network.popover else {
            return;
        };
        for (way, field, input) in popover.inputs.clone() {
            let values = match way {
                Way::Inbound => shown.inbound,
                Way::Outbound => shown.outbound,
            };
            let text = format(field, field.get(values));
            if input.read(cx).value().trim() != text {
                input.update(cx, |state, cx| state.set_value(text, window, cx));
            }
        }
        cx.notify();
    }

    /// The Test ribbon's Network tile, with the popover under it when open.
    pub(in crate::shell) fn network_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(reason) = super::unsupported() {
            return unavailable_tile("ribbon-network", IconName::Wifi, "Network", reason)
                .into_any_element();
        }
        let network = &self.play.network;
        let pill = network.shown().pill();
        tile(
            &self.ribbon_nav,
            "ribbon-network",
            IconName::Wifi,
            "Network",
            cx,
        )
        .tooltip({
            let name = network.shown().name();
            move |window, cx| {
                crate::shell::tooltip::text(format!("Network simulation: {name}"), window, cx)
            }
        })
        .on_click(cx.listener(|shell, _, window, cx| {
            // A click on the tile while open first reaches the
            // popover's click-outside, which has just closed it; that
            // click means "close", not "open again".
            let just_closed = shell
                .play
                .network
                .closed_at
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(300));
            if shell.play.network.popover.is_some() {
                shell.network_close(cx);
            } else if !just_closed {
                shell.network_open(window, cx);
            }
        }))
        .child(div().absolute().top(px(2.)).right(px(2.)).child(ui::pill(
            pill,
            None,
            pill != "Off",
        )))
        .when_some(network.popover.as_ref(), |this, popover| {
            this.child(div().absolute().left_0().top_full().child(deferred(
                anchored().child(self.network_popover(popover, cx)),
            )))
        })
        .into_any_element()
    }

    fn network_popover(&self, popover: &Popover, cx: &mut Context<Self>) -> impl IntoElement {
        let network = &self.play.network;
        let shown = network.shown();
        let section = |title: &'static str, hint: &'static str| {
            h_flex()
                .gap(px(6.))
                .items_baseline()
                .child(
                    ui::text(12., 16.)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(ui::text(10.5, 14.).text_color(tokens::text3()).child(hint))
        };
        let group = |way: Way| {
            v_flex().gap(px(4.)).children(
                popover
                    .inputs
                    .iter()
                    .filter(move |(w, ..)| *w == way)
                    .map(|(_, field, input)| {
                        let (label, unit) = ROWS
                            .iter()
                            .find(|(f, ..)| f == field)
                            .map(|(_, l, u)| (*l, *u))
                            .unwrap_or_default();
                        h_flex()
                            .h(px(28.))
                            .gap(px(8.))
                            .items_center()
                            .child(
                                ui::text(12., 16.)
                                    .flex_1()
                                    .text_color(tokens::text2())
                                    .child(label),
                            )
                            .child(
                                h_flex()
                                    .w(px(96.))
                                    .h(px(26.))
                                    .px(px(8.))
                                    .items_center()
                                    .rounded(px(5.))
                                    .border_1()
                                    .border_color(tokens::border())
                                    .bg(ui::panel())
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            Input::new(input)
                                                .appearance(false)
                                                .px_0()
                                                .text_size(px(12.)),
                                        ),
                                    )
                                    .child(
                                        ui::text(11., 14.).text_color(tokens::text3()).child(unit),
                                    ),
                            )
                    }),
            )
        };
        let preset_button = h_flex()
            .id("network-preset")
            .h(px(28.))
            .px(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded(px(5.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel())
            .cursor_pointer()
            .child(ui::text(12., 16.).flex_1().truncate().child(shown.name()))
            .child(Icon::new(IconName::ChevronDown).size(px(12.)))
            .on_click(cx.listener(|shell, _, _, cx| {
                if let Some(popover) = &mut shell.play.network.popover {
                    popover.menu_open = !popover.menu_open;
                }
                cx.notify();
            }));
        let menu = popover.menu_open.then(|| self.network_menu(cx));
        let is_saved = matches!(shown.choice, Choice::Saved(_));
        let reset = if is_saved {
            ui::button("network-delete", "Delete", Weight::Danger, true).on_click(cx.listener(
                |shell, _, window, cx| {
                    if shell.play.network.delete() {
                        shell.save_settings();
                        shell.network_apply(cx);
                        shell.network_sync_inputs(window, cx);
                    }
                },
            ))
        } else {
            ui::button("network-reset", "Reset", Weight::Ghost, true).on_click(cx.listener(
                |shell, _, window, cx| {
                    shell.play.network.reset();
                    shell.network_sync_inputs(window, cx);
                },
            ))
        };
        let buttons = h_flex()
            .gap(px(8.))
            .child(reset)
            .child(div().flex_1())
            .child(
                ui::button("network-save", "Save", Weight::Secondary, true).on_click(
                    cx.listener(|shell, _, window, cx| shell.network_start_save(window, cx)),
                ),
            )
            .child(
                ui::button("network-apply", "Apply", Weight::Primary, true).on_click(cx.listener(
                    |shell, _, window, cx| {
                        shell.network_apply(cx);
                        shell.network_sync_inputs(window, cx);
                    },
                )),
            );
        let save_row = popover.saving.as_ref().map(|input| {
            let replaces = network.check_name(&input.read(cx).value()) == Ok(true);
            v_flex()
                .gap(px(4.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .h(px(28.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .rounded(px(5.))
                                .border_1()
                                .border_color(if popover.save_error.is_some() {
                                    ui::red()
                                } else {
                                    tokens::accent_line()
                                })
                                .bg(ui::panel())
                                .child(
                                    Input::new(input)
                                        .appearance(false)
                                        .px_0()
                                        .text_size(px(12.)),
                                ),
                        )
                        .child(
                            ui::button(
                                "network-save-apply",
                                if replaces {
                                    "Replace & Apply"
                                } else {
                                    "Save & Apply"
                                },
                                Weight::Primary,
                                true,
                            )
                            .on_click(cx.listener(
                                |shell, _, window, cx| shell.network_finish_save(window, cx),
                            )),
                        ),
                )
                .when_some(popover.save_error, |this, error| {
                    this.child(ui::text(11., 14.).text_color(ui::red()).child(error))
                })
        });
        v_flex()
            .id("network-popover")
            .occlude()
            .mt(px(6.))
            .w(px(WIDTH))
            .gap(px(10.))
            .p(px(14.))
            .border_1()
            .border_color(tokens::border2())
            .rounded(px(10.))
            .bg(tokens::field_select())
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.35),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .text_color(tokens::text())
            .on_mouse_down_out(cx.listener(|shell, _, _, cx| shell.network_close(cx)))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(ui::text(13., 18.).flex_1().font_weight(FontWeight::SEMIBOLD).child("Network simulation"))
                    .child(ui::pill(shown.pill(), None, shown.choice != Choice::Off)),
            )
            .child(section("Preset", ""))
            .child(preset_button)
            .children(menu)
            .child(section("Inbound", "server \u{2192} client"))
            .child(group(Way::Inbound))
            .child(section("Outbound", "client \u{2192} server"))
            .child(group(Way::Outbound))
            .child(
                ui::text(10.5, 14.).text_color(tokens::text3()).child(
                    "Shapes the Roblox client\u{2019}s UDP game traffic, so the game itself lags. \
                     Downloads (TCP) and this editor\u{2019}s live view are not slowed. Needs Sober.",
                ),
            )
            .when_some(popover.status.clone(), |this, status| {
                this.child(ui::text(11., 14.).text_color(tokens::text2()).child(status))
            })
            .children(save_row)
            .child(buttons)
            .child(gpui_base::occlusion::probe())
    }
}
