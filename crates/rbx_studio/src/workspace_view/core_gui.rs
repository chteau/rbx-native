//! Play (local)'s default core GUI, drawn over the game view: health,
//! the backpack hotbar, the player list, notifications and the message a
//! kicked player gets. Our own look, made from the sim's state
//! (`rbx_sim::gui::CoreGui`); nothing here copies Roblox's CoreGuis. Each
//! part shows only while the game leaves its `CoreGuiType` on.

use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_sim::gui::CoreGui;

use super::WorkspaceView;

/// How long a `SendNotification` stays up.
const TOAST: Duration = Duration::from_secs(5);
/// The game's top bar inset (`GuiService:GetGuiInset()`), which the core
/// GUI's top row sits in.
const TOPBAR: f32 = 58.0;
const NUMBER_KEYS: [&str; 10] = [
    "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Zero",
];

fn panel() -> Div {
    div()
        .bg(rgba(0x16181ccc))
        .border_1()
        .border_color(rgba(0xffffff1f))
        .rounded(px(8.))
        .text_color(rgb(0xf2f3f5))
        .text_size(px(12.))
}

/// What the overlay holds between frames.
#[derive(Default)]
pub(crate) struct CoreGuiView {
    pub(crate) state: Option<CoreGui>,
    toasts: Vec<(String, String, Instant)>,
    /// The message shown once the viewed player was kicked.
    pub(crate) kicked: Option<String>,
}

impl CoreGuiView {
    /// The sim's latest state; its notifications join the ones still up.
    pub(crate) fn update(&mut self, mut state: CoreGui) {
        let now = Instant::now();
        self.toasts
            .retain(|(_, _, at)| now.duration_since(*at) < TOAST);
        for (title, text) in std::mem::take(&mut state.notifications) {
            self.toasts.push((title, text, now));
        }
        self.state = Some(state);
    }

    pub(crate) fn clear(&mut self) {
        *self = CoreGuiView::default();
    }
}

fn health(health: f64, max: f64) -> Div {
    let share = if max > 0.0 {
        (health / max).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    // Green to amber to red as it drops.
    let colour = if share > 0.5 {
        rgb(0x3ecf6e)
    } else if share > 0.2 {
        rgb(0xf0b232)
    } else {
        rgb(0xe5484d)
    };
    panel()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .h(px(30.))
        .child(div().child("\u{2665}").text_color(colour))
        .child(
            div()
                .w(px(120.))
                .h(px(8.))
                .rounded_full()
                .bg(rgba(0xffffff26))
                .child(div().h_full().w(relative(share)).rounded_full().bg(colour)),
        )
        .child(
            div()
                .min_w(px(28.))
                .child(format!("{:.0}", health.max(0.0))),
        )
}

fn player_list(players: &[(String, Option<[u8; 3]>)]) -> Div {
    panel()
        .w(px(180.))
        .py_1()
        .child(
            div()
                .px_3()
                .py_1()
                .text_color(rgba(0xf2f3f599))
                .text_size(px(11.))
                .child(format!("Players \u{b7} {}", players.len())),
        )
        .children(players.iter().map(|(name, team)| {
            let dot = team.map_or(rgba(0xffffff40), |[r, g, b]| {
                rgba(u32::from(r) << 24 | u32::from(g) << 16 | u32::from(b) << 8 | 0xff)
            });
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_0p5()
                .child(div().size(px(8.)).rounded_full().bg(dot))
                .child(name.clone())
        }))
}

fn toast(title: &str, text: &str) -> Div {
    panel()
        .w(px(240.))
        .px_3()
        .py_2()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_owned()),
        )
        .when(!text.is_empty(), |this| {
            this.child(div().text_color(rgba(0xf2f3f5b3)).child(text.to_owned()))
        })
}

fn kicked(message: &str) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(0x00000080))
        .child(
            panel()
                .w(px(340.))
                .p_4()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Disconnected"),
                )
                .child(
                    div()
                        .text_color(rgba(0xf2f3f5cc))
                        .child(if message.is_empty() {
                            "You were kicked from this game.".to_owned()
                        } else {
                            format!("You were kicked from this game: {message}")
                        }),
                )
                .child(
                    div()
                        .text_color(rgba(0xf2f3f580))
                        .text_size(px(11.))
                        .child("Stop the run to go back to editing."),
                ),
        )
}

impl WorkspaceView {
    /// The core GUI over the view, while Play (local) shows a client.
    pub(super) fn core_gui_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let core = &self.core_gui;
        if core.state.is_none() && core.kicked.is_none() {
            return None;
        }
        let mut root = div().absolute().inset_0();
        if let Some(state) = &core.state {
            let mut right = div()
                .absolute()
                .top(px((TOPBAR - 30.) / 2.))
                .right(px(12.))
                .flex()
                .flex_col()
                .items_end()
                .gap_2();
            if let Some((now, max)) = state.health {
                right = right.child(health(now, max));
            }
            if let Some(players) = &state.players {
                right = right.child(player_list(players));
            }
            root = root.child(right);
            if let Some(hotbar) = state.hotbar.as_ref().filter(|h| !h.is_empty()) {
                let slots = hotbar.iter().take(10).enumerate().map(|(i, (name, held))| {
                    let key = NUMBER_KEYS[i];
                    div()
                        .id(("core-hotbar", i))
                        .size(px(60.))
                        .relative()
                        .flex()
                        .items_center()
                        .justify_center()
                        .px_1()
                        .rounded(px(8.))
                        .bg(rgba(0x16181ccc))
                        .border_2()
                        .border_color(if *held {
                            rgb(0x5aa9ff)
                        } else {
                            rgba(0xffffff1f)
                        })
                        .text_color(rgb(0xf2f3f5))
                        .text_size(px(11.))
                        .cursor_pointer()
                        .child(
                            div()
                                .absolute()
                                .top(px(3.))
                                .left(px(5.))
                                .text_size(px(10.))
                                .text_color(rgba(0xf2f3f599))
                                .child(format!("{}", (i + 1) % 10)),
                        )
                        .child(div().text_center().overflow_hidden().child(name.clone()))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                if let Some(play) = view.play.as_mut() {
                                    play.tap(key);
                                }
                            }),
                        )
                });
                root = root.child(
                    div()
                        .absolute()
                        .bottom(px(14.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .gap_1p5()
                        .children(slots),
                );
            }
        }
        if !core.toasts.is_empty() {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(90.))
                    .right(px(12.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(core.toasts.iter().map(|(t, x, _)| toast(t, x))),
            );
        }
        if let Some(message) = &core.kicked {
            root = root.child(kicked(message));
        }
        Some(root.into_any_element())
    }
}
