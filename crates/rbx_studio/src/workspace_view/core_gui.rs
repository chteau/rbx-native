//! Play (local)'s default core GUI, drawn over the game view: health,
//! the backpack hotbar, the player list with `leaderstats`, notifications,
//! the chat window and bubbles, the Escape menu (Reset, Leave, volume) and
//! the message a kicked player gets. Our own look, made from the sim's
//! state (`rbx_sim::gui::CoreGui`); nothing here copies Roblox's CoreGuis.
//! Each part shows only while the game leaves its `CoreGuiType` on.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use glam::Vec2;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_lua::server::ChatLine;
use rbx_sim::gui::{CoreGui, PlayerRow};

use super::WorkspaceView;

/// How long a `SendNotification` stays up.
const TOAST: Duration = Duration::from_secs(5);
/// How long a chat bubble stays over a head.
const BUBBLE: Duration = Duration::from_secs(8);
/// Chat lines kept in the window.
const CHAT_LINES: usize = 60;
const ACCENT: u32 = 0x5aa9ff;
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

/// What the player asked of the run through the core GUI, for the shell.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CoreAction {
    Say(String),
    Reset,
    Leave,
    Volume(f32),
    /// The test purchase dialog closed: bought or cancelled.
    Purchase(bool),
}

/// What the overlay holds between frames.
pub(crate) struct CoreGuiView {
    pub(crate) state: Option<CoreGui>,
    toasts: Vec<(String, String, Instant)>,
    /// The message shown once the viewed player was kicked.
    pub(crate) kicked: Option<String>,
    /// The test purchase dialog (title, line) a script's prompt opened.
    pub(crate) purchase: Option<(String, String)>,
    /// The Escape menu is open.
    menu: bool,
    /// What the player is typing into the chat bar, while it has the keys.
    draft: Option<String>,
    chat: Vec<ChatLine>,
    /// Each speaker's latest bubble, by user id.
    bubbles: HashMap<i64, (String, Instant)>,
    /// The run's volume, 0 to 1.
    pub(crate) volume: f32,
    actions: Vec<CoreAction>,
}

impl Default for CoreGuiView {
    fn default() -> Self {
        CoreGuiView {
            state: None,
            toasts: Vec::new(),
            kicked: None,
            purchase: None,
            menu: false,
            draft: None,
            chat: Vec::new(),
            bubbles: HashMap::new(),
            volume: 0.8,
            actions: Vec::new(),
        }
    }
}

/// A message's text without its rich-text tags (a system message comes
/// wrapped in `<font …>`).
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in text.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// A speaker's name colour, steady per name.
fn name_colour(name: &str) -> Rgba {
    const COLOURS: [u32; 8] = [
        0x7cc4ff, 0xffb86b, 0x9be37a, 0xff8fb1, 0xc8a6ff, 0x6fe3d4, 0xffd86b, 0xff9a8a,
    ];
    let hash = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    rgb(COLOURS[hash as usize % COLOURS.len()])
}

impl CoreGuiView {
    /// What the shell should do, since the last call.
    pub(crate) fn take_actions(&mut self) -> Vec<CoreAction> {
        std::mem::take(&mut self.actions)
    }

    /// Messages the viewed client showed: into the window, and a bubble
    /// over the speaker.
    pub(crate) fn push_chat(&mut self, lines: Vec<ChatLine>) {
        let now = Instant::now();
        for line in lines {
            if line.user_id != 0 {
                self.bubbles.insert(line.user_id, (plain(&line.text), now));
            }
            self.chat.push(line);
        }
        let extra = self.chat.len().saturating_sub(CHAT_LINES);
        self.chat.drain(..extra);
        self.bubbles
            .retain(|_, (_, at)| now.duration_since(*at) < BUBBLE);
    }

    /// A key while the game view has the keyboard: the menu and the chat
    /// bar take what they need before the game sees it. `true` when it
    /// was theirs.
    pub(crate) fn key(&mut self, keystroke: &Keystroke, pressed: bool) -> bool {
        if self.state.is_none() {
            return false;
        }
        let key = keystroke.key.as_str();
        if let Some(draft) = self.draft.as_mut() {
            if pressed {
                match key {
                    "escape" => self.draft = None,
                    "enter" => {
                        let text = draft.trim().to_owned();
                        self.draft = None;
                        if !text.is_empty() {
                            self.actions.push(CoreAction::Say(text));
                        }
                    }
                    "backspace" => {
                        draft.pop();
                    }
                    "space" => draft.push(' '),
                    _ => {
                        if let Some(typed) = keystroke
                            .key_char
                            .as_ref()
                            .filter(|c| !c.chars().any(char::is_control))
                        {
                            draft.push_str(typed);
                        }
                    }
                }
            }
            return true;
        }
        if self.menu {
            if pressed {
                match key {
                    "escape" => self.menu = false,
                    "r" => self.reset(),
                    "l" => self.actions.push(CoreAction::Leave),
                    _ => {}
                }
            }
            return true;
        }
        match key {
            "escape" => {
                if pressed {
                    self.menu = true;
                }
                true
            }
            "/" if pressed => {
                self.draft = Some(String::new());
                true
            }
            _ => false,
        }
    }

    /// Whether the Escape menu is up (the game's `GuiService.MenuIsOpen`).
    pub(crate) fn menu_open(&self) -> bool {
        self.menu
    }

    /// Whether the menu or the chat bar holds the keyboard (the game's
    /// held keys are let go).
    pub(crate) fn has_keys(&self) -> bool {
        self.menu || self.draft.is_some()
    }

    fn reset(&mut self) {
        if self.state.as_ref().is_some_and(|s| s.can_reset) {
            self.menu = false;
            self.actions.push(CoreAction::Reset);
        }
    }

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
        let volume = self.volume;
        *self = CoreGuiView {
            volume,
            ..CoreGuiView::default()
        };
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

/// The player list; with `leaderstats`, a column per stat.
fn player_list(players: &[PlayerRow], stats: &[String]) -> Div {
    const STAT: f32 = 64.;
    let header = div()
        .flex()
        .items_center()
        .px_3()
        .py_1()
        .text_color(rgba(0xf2f3f599))
        .text_size(px(11.))
        .child(
            div()
                .flex_1()
                .child(format!("Players \u{b7} {}", players.len())),
        )
        .children(stats.iter().map(|name| {
            div()
                .w(px(STAT))
                .text_right()
                .overflow_hidden()
                .child(name.clone())
        }));
    panel()
        .w(px(180. + STAT * stats.len() as f32))
        .py_1()
        .child(header)
        .children(players.iter().map(|row| {
            let dot = row.team.map_or(rgba(0xffffff40), |[r, g, b]| {
                rgba(u32::from(r) << 24 | u32::from(g) << 16 | u32::from(b) << 8 | 0xff)
            });
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_0p5()
                .child(div().size(px(8.)).rounded_full().bg(dot))
                .child(div().flex_1().overflow_hidden().child(row.name.clone()))
                .children(row.stats.iter().map(|value| {
                    div()
                        .w(px(STAT))
                        .text_right()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(value.clone())
                }))
        }))
}

/// The chat window, top left under the bar: recent lines, then the bar
/// the player types into (`/` to start, Enter to send).
fn chat_window(lines: &[ChatLine], draft: Option<&str>) -> Div {
    let shown = lines.iter().rev().take(8).rev();
    panel()
        .w(px(340.))
        .p_2()
        .flex()
        .flex_col()
        .gap_0p5()
        .bg(rgba(0x16181ca6))
        .children(shown.map(|line| {
            let text = plain(&line.text);
            let row = div().flex().flex_wrap().gap_1().line_height(px(17.));
            if line.prefix.is_empty() {
                row.child(div().text_color(rgba(0xf2f3f5b3)).child(text))
            } else {
                row.child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(name_colour(&line.prefix))
                        .child(line.prefix.clone()),
                )
                .child(text)
            }
        }))
        .child(
            div()
                .mt_1()
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .rounded(px(6.))
                .bg(rgba(0x0000004d))
                .border_1()
                .border_color(if draft.is_some() {
                    rgb(ACCENT)
                } else {
                    rgba(0xffffff14)
                })
                .child(match draft {
                    Some(text) => div().child(format!("{text}\u{258f}")),
                    None => div().text_color(rgba(0xf2f3f566)).child("Press / to chat"),
                }),
        )
}

/// A speech bubble, its tail under it, centred on `at`'s x and standing
/// on it.
fn bubble(text: &str, at: Point<Pixels>) -> Div {
    div()
        .absolute()
        .left(at.x - px(110.))
        .top(at.y - px(64.))
        .w(px(220.))
        .h(px(64.))
        .flex()
        .flex_col()
        .justify_end()
        .items_center()
        .child(
            div()
                .max_w(px(220.))
                .px_3()
                .py_1p5()
                .rounded(px(12.))
                .bg(rgba(0xf7f8faf0))
                .text_color(rgb(0x1b1d22))
                .text_size(px(13.))
                .text_center()
                .child(text.to_owned()),
        )
        .child(
            div()
                .size(px(8.))
                .mt(px(-4.))
                .bg(rgba(0xf7f8faf0))
                .rounded(px(2.)),
        )
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
    /// `point` on the view in logical pixels, or `None` behind the camera.
    fn project_world(&self, point: glam::Vec3, scale: f32) -> Option<Point<Pixels>> {
        let pose = self.view?;
        let viewport = self.viewport.get();
        let size = Vec2::new(viewport.size.0 as f32, viewport.size.1 as f32);
        if size.min_element() <= 0.0 {
            return None;
        }
        let clip = pose.view_projection(self.orthographic, size.x / size.y) * point.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = Vec2::new(clip.x, clip.y) / clip.w;
        let pixel = Vec2::new((ndc.x + 1.0) * 0.5 * size.x, (1.0 - ndc.y) * 0.5 * size.y);
        (pixel.cmpge(Vec2::ZERO).all() && pixel.cmple(size).all())
            .then(|| Point::new(px(pixel.x / scale), px(pixel.y / scale)))
    }

    /// The Escape menu: Resume, Reset character, Leave and the volume.
    fn core_menu(&self, cx: &mut Context<Self>) -> Div {
        let core = &self.core_gui;
        let can_reset = core.state.as_ref().is_some_and(|s| s.can_reset);
        let button = |id: &'static str, label: &'static str, hint: &'static str, enabled: bool| {
            div()
                .id(id)
                .h(px(40.))
                .px_4()
                .flex()
                .items_center()
                .justify_between()
                .rounded(px(8.))
                .bg(rgba(0xffffff0f))
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .hover(|this| this.bg(rgba(0xffffff1f)))
                })
                .when(!enabled, |this| this.text_color(rgba(0xf2f3f559)))
                .child(label)
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgba(0xf2f3f580))
                        .child(hint),
                )
        };
        let volume = core.volume;
        let segments = (1..=10usize).map(|i| {
            let level = i as f32 / 10.;
            div()
                .id(("core-volume", i))
                .flex_1()
                .h(px(14.))
                .rounded(px(3.))
                .cursor_pointer()
                .bg(if level <= volume + 0.001 {
                    rgb(ACCENT)
                } else {
                    rgba(0xffffff1f)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let core = &mut view.core_gui;
                        core.volume = if (core.volume - level).abs() < 0.001 && i == 1 {
                            0.
                        } else {
                            level
                        };
                        core.actions.push(CoreAction::Volume(core.volume));
                        cx.notify();
                    }),
                )
        });
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x0000008c))
            .child(
                panel()
                    .w(px(320.))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_size(px(13.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .mb_1()
                            .child("Game menu"),
                    )
                    .child(button("core-resume", "Resume", "Esc", true).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            view.core_gui.menu = false;
                            cx.notify();
                        }),
                    ))
                    .child(
                        button("core-reset", "Reset character", "R", can_reset).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                view.core_gui.reset();
                                cx.notify();
                            }),
                        ),
                    )
                    .child(button("core-leave", "Leave game", "L", true).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            view.core_gui.actions.push(CoreAction::Leave);
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().w(px(56.)).child("Volume"))
                            .child(div().flex_1().flex().gap_1().children(segments))
                            .child(
                                div()
                                    .w(px(34.))
                                    .text_right()
                                    .text_color(rgba(0xf2f3f5b3))
                                    .child(format!("{:.0}%", volume * 100.)),
                            ),
                    ),
            )
    }

    /// The test purchase dialog: Buy or Cancel, never real Robux.
    fn purchase_dialog(&self, title: &str, line: &str, cx: &mut Context<Self>) -> Div {
        let button = |id: &'static str, label: &'static str, bought: bool| {
            div()
                .id(id)
                .h(px(36.))
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .when(bought, |this| {
                    this.bg(rgb(ACCENT)).text_color(rgb(0x0b1220))
                })
                .when(!bought, |this| {
                    this.bg(rgba(0xffffff0f))
                        .hover(|this| this.bg(rgba(0xffffff1f)))
                })
                .child(label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        view.core_gui.purchase = None;
                        view.core_gui.actions.push(CoreAction::Purchase(bought));
                        cx.notify();
                    }),
                )
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x0000008c))
            .child(
                panel()
                    .w(px(340.))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .text_size(px(13.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.to_owned()),
                    )
                    .child(div().text_color(rgba(0xf2f3f5cc)).child(line.to_owned()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(button("core-purchase-cancel", "Cancel", false))
                            .child(button("core-purchase-buy", "Buy (test)", true)),
                    ),
            )
    }

    /// The core GUI over the view, while Play (local) shows a client.
    pub(super) fn core_gui_element(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let core = &self.core_gui;
        if core.state.is_none() && core.kicked.is_none() && core.purchase.is_none() {
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
                right = right.child(player_list(players, &state.stats));
            }
            root = root.child(right);
            let scale = window.scale_factor();
            let now = Instant::now();
            for (user_id, head) in &state.heads {
                let Some((text, _)) = core
                    .bubbles
                    .get(user_id)
                    .filter(|(_, at)| now.duration_since(*at) < BUBBLE)
                else {
                    continue;
                };
                let top = glam::Vec3::from(*head) + glam::Vec3::Y * 1.6;
                if let Some(at) = self.project_world(top, scale) {
                    root = root.child(bubble(text, at));
                }
            }
            root = root.child(
                div()
                    .absolute()
                    .top(px(TOPBAR + 6.))
                    .left(px(12.))
                    .child(chat_window(&core.chat, core.draft.as_deref())),
            );
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
        if core.menu && core.state.is_some() {
            root = root.child(self.core_menu(cx));
        }
        if let Some((title, line)) = &core.purchase {
            root = root.child(self.purchase_dialog(title, line, cx));
        }
        if let Some(message) = &core.kicked {
            root = root.child(kicked(message));
        }
        Some(root.into_any_element())
    }
}
