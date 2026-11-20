//! Play (local)'s default core GUI, drawn over the game view, laid out as
//! the current Roblox client lays its own (measured off captures of it):
//! the top bar's menu and chat buttons, the People list with `leaderstats`,
//! the chat window and bubbles, the backpack hotbar, health once hurt, the
//! red edge on death, the Escape menu with its respawn and leave
//! confirmations, notifications and the message a kicked player gets.
//! Our own drawing and icons from the sim's state (`rbx_sim::gui::CoreGui`);
//! nothing here copies Roblox's CoreScripts or images. Each part shows only
//! while the game leaves its `CoreGuiType` (and `TopbarEnabled`) on.

mod icons;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use glam::Vec2;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_lua::server::ChatLine;
use rbx_sim::gui::{CoreGui, HotbarSlot, PlayerRow};

use self::icons::{icon, Glyph};
use super::WorkspaceView;

/// How long a `SendNotification` stays up.
const TOAST: Duration = Duration::from_secs(5);
/// How long a chat bubble stays over a head.
const BUBBLE: Duration = Duration::from_secs(8);
/// How long the chat window keeps its background after the last message or
/// keystroke; after that only its lines show.
const CHAT_FADE: Duration = Duration::from_secs(8);
/// Chat lines kept in the window.
const CHAT_LINES: usize = 60;
const NUMBER_KEYS: [&str; 10] = [
    "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Zero",
];
/// The client's UI face, read from the Roblox content the player already
/// has (never shipped with us); the app's own face otherwise.
const FONT: &str = "Builder Sans";
const WHITE: u32 = 0xf7f7f8;
/// The top bar buttons' fill.
const BAR: u32 = 0x191f24;
/// The confirm and Resume buttons' blue.
const BLUE: u32 = 0x1e3072;
/// The equipped slot's outline.
const HELD: u32 = 0x5a8ee9;

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

/// The Escape menu's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    People,
    Settings,
    Gallery,
    Report,
    Help,
}

impl Tab {
    const ALL: [Tab; 5] = [
        Tab::People,
        Tab::Settings,
        Tab::Gallery,
        Tab::Report,
        Tab::Help,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::People => "People",
            Tab::Settings => "Settings",
            Tab::Gallery => "Gallery",
            Tab::Report => "Report",
            Tab::Help => "Help",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Tab::People => Glyph::People,
            Tab::Settings => Glyph::Settings,
            Tab::Gallery => Glyph::Gallery,
            Tab::Report => Glyph::Report,
            Tab::Help => Glyph::Help,
        }
    }
}

/// A question the menu asks before Respawn or Leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Confirm {
    Respawn,
    Leave,
}

/// A tool icon, `None` once it failed to load.
type Icon = Option<Arc<RenderImage>>;

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
    tab: Tab,
    confirm: Option<Confirm>,
    /// The chat window is open (the top bar's chat button).
    chat_open: bool,
    /// When the chat window last had something new: its background shows
    /// for [`CHAT_FADE`] after.
    chat_seen: Option<Instant>,
    /// The People list was closed (its X, or Tab).
    list_hidden: bool,
    /// What the player is typing into the chat bar, while it has the keys.
    draft: Option<String>,
    chat: Vec<ChatLine>,
    /// Each speaker's latest bubble, by user id.
    bubbles: HashMap<i64, (String, Instant)>,
    /// The run's volume, 0 to 1.
    pub(crate) volume: f32,
    actions: Vec<CoreAction>,
    /// Tool icons by `TextureId`, `None` while loading or when the image
    /// can't be had.
    icons: HashMap<String, Icon>,
    /// Icons the loading threads finished, not yet in `icons`.
    arrived: Arc<Mutex<Vec<(String, Icon)>>>,
}

impl Default for CoreGuiView {
    fn default() -> Self {
        CoreGuiView {
            state: None,
            toasts: Vec::new(),
            kicked: None,
            purchase: None,
            menu: false,
            tab: Tab::People,
            confirm: None,
            chat_open: false,
            chat_seen: None,
            list_hidden: false,
            draft: None,
            chat: Vec::new(),
            bubbles: HashMap::new(),
            volume: 0.8,
            actions: Vec::new(),
            icons: HashMap::new(),
            arrived: Arc::default(),
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
        0xfd2943, 0x01a2ff, 0x02b857, 0xa75ec9, 0xda8541, 0xf5cd30, 0xe8bac8, 0xd7c59a,
    ];
    let hash = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    rgb(COLOURS[hash as usize % COLOURS.len()])
}

/// The share of health left, 0 to 1.
fn share(health: f64, max: f64) -> f32 {
    if max > 0.0 {
        (health / max).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
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
        if !lines.is_empty() {
            self.chat_seen = Some(now);
        }
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

    fn chat_allowed(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.chat)
    }

    fn typing_allowed(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.chat_bar)
    }

    /// Opens the chat window with the bar focused.
    fn focus_chat(&mut self) {
        if self.typing_allowed() {
            self.chat_open = true;
            self.draft = Some(String::new());
            self.chat_seen = Some(Instant::now());
        }
    }

    fn toggle_chat(&mut self) {
        self.chat_open = !self.chat_open && self.chat_allowed();
        if !self.chat_open {
            self.draft = None;
        }
        self.chat_seen = Some(Instant::now());
    }

    fn open_menu(&mut self) {
        self.menu = true;
        self.tab = Tab::People;
        self.confirm = None;
        self.draft = None;
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
                self.chat_seen = Some(Instant::now());
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
                match (self.confirm, key) {
                    (Some(_), "escape") => self.confirm = None,
                    (Some(confirm), "enter") => self.confirmed(confirm),
                    (None, "escape") => self.menu = false,
                    (None, "r") => self.ask(Confirm::Respawn),
                    (None, "l") => self.ask(Confirm::Leave),
                    _ => {}
                }
            }
            return true;
        }
        match key {
            "escape" => {
                if pressed {
                    self.open_menu();
                }
                true
            }
            "/" if pressed && self.typing_allowed() => {
                self.focus_chat();
                true
            }
            "tab" => {
                if pressed {
                    self.list_hidden = !self.list_hidden;
                }
                true
            }
            _ => false,
        }
    }

    fn ask(&mut self, confirm: Confirm) {
        if confirm == Confirm::Respawn && !self.can_reset() {
            return;
        }
        self.confirm = Some(confirm);
    }

    fn confirmed(&mut self, confirm: Confirm) {
        self.confirm = None;
        self.menu = false;
        self.actions.push(match confirm {
            Confirm::Respawn => CoreAction::Reset,
            Confirm::Leave => CoreAction::Leave,
        });
    }

    fn can_reset(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.can_reset)
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

    /// The sim's latest state; its notifications join the ones still up.
    pub(crate) fn update(&mut self, mut state: CoreGui) {
        let now = Instant::now();
        self.toasts
            .retain(|(_, _, at)| now.duration_since(*at) < TOAST);
        for (title, text) in std::mem::take(&mut state.notifications) {
            self.toasts.push((title, text, now));
        }
        if !state.chat {
            self.chat_open = false;
        }
        if !state.chat_bar {
            self.draft = None;
        }
        self.load_icons(state.hotbar.as_deref().unwrap_or_default());
        self.state = Some(state);
    }

    /// Starts loading the hotbar's icons not asked for yet, and takes in the
    /// ones that came.
    fn load_icons(&mut self, slots: &[HotbarSlot]) {
        if let Ok(mut arrived) = self.arrived.lock() {
            self.icons.extend(arrived.drain(..));
        }
        for slot in slots {
            if slot.icon.is_empty() || self.icons.contains_key(&slot.icon) {
                continue;
            }
            self.icons.insert(slot.icon.clone(), None);
            let (uri, arrived) = (slot.icon.clone(), self.arrived.clone());
            std::thread::spawn(move || {
                let image = rbx_viewer::asset_bytes(&uri)
                    .ok()
                    .and_then(|bytes| image::load_from_memory(&bytes).ok())
                    .and_then(|image| {
                        let image = image.to_rgba8();
                        let (w, h) = image.dimensions();
                        crate::render_image::to_render_image(image.into_raw(), w, h)
                    });
                if let Ok(mut arrived) = arrived.lock() {
                    arrived.push((uri, image));
                }
            });
        }
    }

    pub(crate) fn clear(&mut self) {
        *self = CoreGuiView {
            volume: self.volume,
            icons: std::mem::take(&mut self.icons),
            arrived: self.arrived.clone(),
            ..CoreGuiView::default()
        };
    }
}

/// Loads [`FONT`] once from the Roblox content on disk; whether it is there.
fn client_font(cx: &mut App) -> bool {
    static LOADED: OnceLock<bool> = OnceLock::new();
    *LOADED.get_or_init(|| {
        let Ok(cache) = rbx_assets::AssetCache::new(None) else {
            return false;
        };
        let faces: Vec<Cow<'static, [u8]>> = ["Regular", "Medium", "Bold"]
            .iter()
            .filter_map(|face| cache.get_native(&format!("fonts/BuilderSans-{face}.otf")))
            .map(Cow::Owned)
            .collect();
        !faces.is_empty() && cx.text_system().add_fonts(faces).is_ok()
    })
}

/// A round dark top bar button.
fn bar_button(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(44.))
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(BAR << 8 | 0xf2))
        .rounded_full()
        .cursor_pointer()
}

/// The health bar, top right, once the character is hurt (as the client
/// shows it): a dark-rimmed grey pill, its fill going green to red (the
/// client's yellow-green at 60% is measured; the ends are by eye).
fn health_colour(share: f32) -> Rgba {
    const STOPS: [(f32, [f32; 3]); 4] = [
        (0.0, [232., 48., 48.]),
        (0.3, [250., 170., 20.]),
        (0.6, [207., 232., 20.]),
        (1.0, [60., 220., 90.]),
    ];
    let i = STOPS
        .iter()
        .rposition(|(at, _)| *at <= share)
        .unwrap_or(0)
        .min(2);
    let ((a, from), (b, to)) = (STOPS[i], STOPS[i + 1]);
    let t = ((share - a) / (b - a)).clamp(0., 1.);
    let [r, g, bl] = std::array::from_fn(|k| (from[k] + (to[k] - from[k]) * t) as u32);
    rgb(r << 16 | g << 8 | bl)
}

fn health(share: f32) -> Div {
    div()
        .absolute()
        .top(px(24.))
        .right(px(16.))
        .w(px(125.))
        .h(px(20.))
        .rounded_full()
        .border(px(3.))
        .border_color(rgba(0x000000b3))
        .bg(rgba(0x646464cc))
        .overflow_hidden()
        .child(div().h_full().w(relative(share)).bg(health_colour(share)))
}

/// The red at the screen's edges while the character is dead.
fn vignette() -> Div {
    let edge = |angle: f32| {
        linear_gradient(
            angle,
            linear_color_stop(rgba(0xe0202e58), 0.),
            linear_color_stop(rgba(0xe0202e00), 1.),
        )
    };
    let side = |this: Div| this.absolute().bg(rgba(0));
    div()
        .absolute()
        .inset_0()
        .child(
            side(div())
                .top_0()
                .left_0()
                .bottom_0()
                .w(relative(0.45))
                .bg(edge(90.)),
        )
        .child(
            side(div())
                .top_0()
                .right_0()
                .bottom_0()
                .w(relative(0.45))
                .bg(edge(270.)),
        )
        .child(
            side(div())
                .top_0()
                .left_0()
                .right_0()
                .h(relative(0.4))
                .bg(edge(180.)),
        )
        .child(
            side(div())
                .bottom_0()
                .left_0()
                .right_0()
                .h(relative(0.4))
                .bg(edge(0.)),
        )
}

/// The People list, top right; with `leaderstats`, a column per stat.
fn player_list(players: &[PlayerRow], stats: &[String], cx: &mut Context<WorkspaceView>) -> Div {
    const STAT: f32 = 66.;
    let row = || div().flex().items_center().pl(px(14.)).pr(px(6.));
    let header = row()
        .h(px(26.))
        .text_size(px(15.))
        .text_color(rgb(0xb8babf))
        .child(div().flex_1().child("People"))
        .children(stats.iter().map(|name| {
            div()
                .w(px(STAT))
                .text_center()
                .overflow_hidden()
                .child(name.clone())
        }));
    div()
        .absolute()
        .top(px(62.))
        .right(px(8.))
        .w(px(186. + STAT * stats.len() as f32))
        .pt(px(4.))
        .pb(px(2.))
        .rounded(px(8.))
        .bg(rgba(0x161619b8))
        .text_color(rgb(WHITE))
        .child(
            div()
                .id("core-list-close")
                .ml(px(6.))
                .size(px(16.))
                .cursor_pointer()
                .child(icon(Glyph::Close, 16.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        view.core_gui.list_hidden = true;
                        cx.notify();
                    }),
                ),
        )
        .child(header)
        .children(players.iter().map(|player| {
            row()
                .h(px(40.))
                .gap_2()
                .text_size(px(17.))
                .when_some(player.team, |this, [r, g, b]| {
                    this.child(
                        div()
                            .size(px(8.))
                            .rounded_full()
                            .bg(rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(player.display_name.clone()),
                )
                .children(
                    player
                        .stats
                        .iter()
                        .map(|value| div().w(px(STAT)).text_center().child(value.clone())),
                )
        }))
}

/// One chat line: the speaker's name in their colour, then the text.
fn chat_line(line: &ChatLine, faded: bool) -> Div {
    let text = plain(&line.text);
    let row = div()
        .flex()
        .flex_wrap()
        .text_size(px(15.))
        .line_height(px(18.))
        .text_color(rgb(WHITE))
        .when(faded, |this| this.opacity(0.9));
    if line.prefix.is_empty() {
        row.child(text)
    } else {
        let name = line.prefix.trim_end_matches(':').to_owned();
        row.child(div().text_color(name_colour(&name)).child(name))
            .child(format!(": {text}"))
    }
}

/// A key cap, as the menu's buttons show their shortcut.
fn key_cap(label: &'static str) -> Div {
    div()
        .h(px(22.))
        .min_w(px(22.))
        .px(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .border_1()
        .border_color(rgb(WHITE))
        .text_size(px(if label.len() > 1 { 9. } else { 12. }))
        .font_weight(FontWeight::BOLD)
        .child(label)
}

/// A wide menu button; `primary` is the client's outlined blue.
fn menu_button(id: &'static str, primary: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(48.))
        .flex()
        .items_center()
        .justify_center()
        .gap_3()
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(px(17.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(WHITE))
        .when(primary, |this| {
            this.bg(rgb(BLUE)).border_2().border_color(rgb(0xffffff))
        })
        .when(!primary, |this| {
            this.bg(rgba(0x2a2c33e6))
                .hover(|this| this.bg(rgba(0x3a3d46e6)))
        })
}

/// A Humanoid's name over its head, its baseline on `at`, the health bar
/// under it: white text with a one-pixel dark edge, as the real client
/// draws it (16 px, edge (51, 58, 64)).
fn name_label(text: &str, health: Option<f32>, at: Point<Pixels>) -> Div {
    const ASCENT: f32 = 14.;
    let line = |colour: u32, dx: f32, dy: f32| {
        div()
            .absolute()
            .left(px(dx))
            .top(px(dy))
            .w_full()
            .text_center()
            .text_color(rgb(colour))
            .child(text.to_owned())
    };
    let mut label = div()
        .absolute()
        .left(at.x - px(150.))
        .top(at.y - px(ASCENT))
        .w(px(300.))
        .h(px(18.))
        .text_size(px(16.))
        .line_height(px(18.));
    if !text.is_empty() {
        for (dx, dy) in [(-1., 0.), (1., 0.), (0., -1.), (0., 1.)] {
            label = label.child(line(0x333a40, dx, dy));
        }
        label = label.child(line(0xffffff, 0., 0.));
    }
    label.when_some(health, |this, share| {
        this.child(
            div()
                .absolute()
                .top(px(ASCENT + 5.))
                .left(px(150. - 30.))
                .w(px(60.))
                .h(px(6.))
                .rounded(px(3.))
                .bg(rgba(0x000000a0))
                .child(
                    div()
                        .h_full()
                        .w(px(60. * share))
                        .rounded(px(3.))
                        .bg(health_colour(share)),
                ),
        )
    })
}

/// A speech bubble, its tail under it, centred on `at`'s x and standing
/// on it.
fn bubble(text: &str, at: Point<Pixels>) -> Div {
    div()
        .absolute()
        .left(at.x - px(150.))
        .top(at.y - px(80.))
        .w(px(300.))
        .h(px(80.))
        .flex()
        .flex_col()
        .justify_end()
        .items_center()
        .child(
            div()
                .max_w(px(300.))
                .px(px(14.))
                .py(px(7.))
                .rounded(px(14.))
                .bg(rgba(0xd4d5d6f5))
                .text_color(rgb(0x4a4c50))
                .text_size(px(17.))
                .text_center()
                .child(text.to_owned()),
        )
        .child(
            div()
                .size(px(9.))
                .mt(px(-5.))
                .bg(rgba(0xd4d5d6f5))
                .rounded(px(1.)),
        )
}

fn toast(title: &str, text: &str) -> Div {
    div()
        .w(px(260.))
        .px_3()
        .py_2()
        .rounded(px(8.))
        .bg(rgba(0x191b1de6))
        .text_color(rgb(WHITE))
        .text_size(px(14.))
        .child(div().font_weight(FontWeight::BOLD).child(title.to_owned()))
        .when(!text.is_empty(), |this| {
            this.child(div().text_color(rgb(0xc4c6ca)).child(text.to_owned()))
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
            div()
                .w(px(400.))
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .rounded(px(8.))
                .bg(rgba(0x191b1df2))
                .text_color(rgb(WHITE))
                .text_size(px(15.))
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::BOLD)
                        .child("Disconnected"),
                )
                .child(
                    div()
                        .text_color(rgb(0xc4c6ca))
                        .child(if message.is_empty() {
                            "You were kicked from this experience.".to_owned()
                        } else {
                            format!("You were kicked from this experience: {message}")
                        }),
                )
                .child(
                    div()
                        .text_color(rgb(0x8e9196))
                        .text_size(px(13.))
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

    /// The top bar: the menu button, then the pill with the menu and chat
    /// toggles.
    fn core_topbar(&self, chat: bool, cx: &mut Context<Self>) -> Div {
        let menu = || {
            cx.listener(|view: &mut Self, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if view.core_gui.menu {
                    view.core_gui.menu = false;
                } else {
                    view.core_gui.open_menu();
                }
                cx.notify();
            })
        };
        div()
            .absolute()
            .top(px(12.))
            .left(px(12.))
            .flex()
            .gap(px(12.))
            .child(
                bar_button("core-logo")
                    .w(px(44.))
                    .child(icon(Glyph::Logo, 26.))
                    .on_mouse_down(MouseButton::Left, menu()),
            )
            .child(
                bar_button("core-pill")
                    .px(px(4.))
                    .child(
                        div()
                            .id("core-menu")
                            .size(px(44.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(Glyph::Menu, 24.))
                            .on_mouse_down(MouseButton::Left, menu()),
                    )
                    .when(chat, |this| {
                        let open = self.core_gui.chat_open;
                        this.child(
                            div()
                                .id("core-chat")
                                .size(px(44.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(if open { Glyph::ChatOn } else { Glyph::Chat }, 24.))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        view.core_gui.toggle_chat();
                                        cx.notify();
                                    }),
                                ),
                        )
                    }),
            )
    }

    /// The chat window under the top bar: its channel tabs, the lines and
    /// the bar. Its background fades once nothing happened for a while,
    /// leaving the lines.
    fn core_chat(&self, typing: bool, cx: &mut Context<Self>) -> Div {
        let core = &self.core_gui;
        let focused = core.draft.is_some();
        let lit = focused || core.chat_seen.is_some_and(|at| at.elapsed() < CHAT_FADE);
        let lines = div()
            .flex_1()
            .min_h_0()
            .px(px(8.))
            .pt(px(8.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(chat_line(
                        &ChatLine {
                            prefix: String::new(),
                            text: "\u{1f44b} Say hi to everyone playing now!".into(),
                            channel: String::new(),
                            user_id: 0,
                        },
                        !lit,
                    ))
                    .when(lit, |this| {
                        this.child(div().text_color(rgb(WHITE)).child("\u{b7}\u{b7}\u{b7}"))
                    }),
            )
            .children(
                core.chat
                    .iter()
                    .rev()
                    .take(12)
                    .rev()
                    .map(|l| chat_line(l, !lit)),
            );
        let tabs =
            div()
                .h(px(40.))
                .flex()
                .rounded_t(px(8.))
                .bg(rgb(0x191b1d))
                .text_size(px(17.))
                .font_weight(FontWeight::SEMIBOLD)
                .children(["Here", "Global", "Friends"].into_iter().enumerate().map(
                    |(i, name)| {
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(i > 0, |this| {
                                this.border_l_1().border_color(rgba(0xffffff26))
                            })
                            .when(i == 0, |this| {
                                this.text_color(rgb(0xffffff))
                                    .border_b(px(3.))
                                    .border_color(rgb(0xffffff))
                            })
                            .when(i > 0, |this| this.text_color(rgb(0xafafaf)))
                            .child(name)
                    },
                ));
        let bar = div()
            .h(px(52.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .size(px(30.))
                    .rounded_full()
                    .bg(rgba(0x00000040))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(Glyph::Smile, 24.)),
            )
            .child(
                div()
                    .id("core-chat-bar")
                    .flex_1()
                    .h(px(38.))
                    .px(px(20.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(rgba(0x0e1012b3))
                    .border_1()
                    .border_color(if focused {
                        rgba(0xffffff80)
                    } else {
                        rgba(0xffffff14)
                    })
                    .text_size(px(15.))
                    .cursor_text()
                    .child(match core.draft.as_deref() {
                        Some(text) => div()
                            .text_color(rgb(WHITE))
                            .child(format!("{text}\u{258f}")),
                        None => div()
                            .text_color(rgb(0x8e9196))
                            .child("To chat click here or press Slash key"),
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            view.core_gui.focus_chat();
                            cx.notify();
                        }),
                    ),
            );
        div()
            .absolute()
            .top(px(62.))
            .left(px(8.))
            .w(px(475.))
            .h(px(298.))
            .flex()
            .flex_col()
            .rounded(px(8.))
            .when(lit, |this| {
                this.bg(linear_gradient(
                    180.,
                    linear_color_stop(rgba(0x191b1dbf), 0.),
                    linear_color_stop(rgba(0x191b1d8c), 1.),
                ))
                .child(tabs)
            })
            .when(!lit, |this| this.pt(px(40.)))
            .child(lines)
            .when(lit && typing, |this| this.child(bar))
            .when(!lit || !typing, |this| this.pb(px(10.)))
    }

    /// The Escape menu: its tabs over the People page (or Settings'
    /// volume), Leave, Respawn and Resume along the bottom; or, once asked,
    /// the respawn or leave question in its place.
    fn core_menu(&self, cx: &mut Context<Self>) -> Div {
        let core = &self.core_gui;
        let can_reset = core.can_reset();
        let panel = div()
            .h_full()
            .w(px(840.))
            .max_w(relative(0.94))
            .rounded(px(10.))
            .bg(rgba(0x000000b3))
            .text_color(rgb(WHITE))
            .flex()
            .flex_col();
        let backdrop = div()
            .absolute()
            .inset_0()
            .pt(px(72.))
            .pb(px(52.))
            .flex()
            .justify_center()
            .bg(rgba(0x00000080))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()),
            );
        if let Some(confirm) = core.confirm {
            let (question, yes, no, yes_id, no_id) = match confirm {
                Confirm::Respawn => (
                    "Are you sure you want to respawn your character?",
                    "Respawn",
                    "Don't Respawn",
                    "core-confirm-respawn",
                    "core-cancel-respawn",
                ),
                Confirm::Leave => (
                    "Are you sure you want to leave the experience?",
                    "Leave",
                    "Don't Leave",
                    "core-confirm-leave",
                    "core-cancel-leave",
                ),
            };
            return backdrop.child(
                panel
                    .items_center()
                    .pt(px(158.))
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(question),
                    )
                    .child(
                        div()
                            .mt(px(52.))
                            .flex()
                            .gap(px(12.))
                            .child(
                                menu_button(yes_id, true)
                                    .w(px(212.))
                                    .h(px(52.))
                                    .font_weight(FontWeight::NORMAL)
                                    .child(yes)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            view.core_gui.confirmed(confirm);
                                            cx.notify();
                                        }),
                                    ),
                            )
                            .child(
                                menu_button(no_id, false)
                                    .w(px(202.))
                                    .h(px(52.))
                                    .font_weight(FontWeight::NORMAL)
                                    .bg(rgba(0))
                                    .border_1()
                                    .border_color(rgb(0xffffff))
                                    .child(no)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            view.core_gui.confirm = None;
                                            cx.notify();
                                        }),
                                    ),
                            ),
                    ),
            );
        }
        let tabs = div()
            .mx(px(24.))
            .h(px(60.))
            .flex()
            .border_b_1()
            .border_color(rgba(0xffffff26))
            .children(Tab::ALL.into_iter().map(|tab| {
                let on = tab == core.tab;
                div()
                    .id(("core-tab", tab as usize))
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_size(px(18.))
                    .cursor_pointer()
                    .text_color(if on { rgb(0xffffff) } else { rgb(0xa9abaf) })
                    .when(on, |this| {
                        this.font_weight(FontWeight::SEMIBOLD)
                            .border_b(px(2.))
                            .border_color(rgb(0xffffff))
                    })
                    .child(icon(tab.glyph(), 22.).when(!on, |this| this.opacity(0.7)))
                    .child(tab.label())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            view.core_gui.tab = tab;
                            cx.notify();
                        }),
                    )
            }));
        let page = match core.tab {
            Tab::People => self.menu_people(),
            Tab::Settings => self.menu_settings(cx),
            tab => div().px(px(32.)).pt(px(24.)).text_size(px(16.)).text_color(rgb(0xc4c6ca)).child(
                match tab {
                    Tab::Gallery => "Captures aren't kept in Local Run.",
                    Tab::Report => "There's no one to report in Local Run.",
                    _ => "Move with WASD or the arrow keys, jump with Space, look around with the right mouse button and zoom with the wheel. / chats, Tab shows People, Escape opens this menu.",
                },
            ),
        };
        let buttons = div()
            .px(px(20.))
            .pb(px(20.))
            .flex()
            .gap(px(9.))
            .child(
                menu_button("core-leave", false)
                    .flex_1()
                    .child(key_cap("L"))
                    .child("Leave")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            view.core_gui.ask(Confirm::Leave);
                            cx.notify();
                        }),
                    ),
            )
            .child(
                menu_button("core-reset", false)
                    .flex_1()
                    .when(!can_reset, |this| this.opacity(0.4).cursor_default())
                    .child(key_cap("R"))
                    .child("Respawn")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            view.core_gui.ask(Confirm::Respawn);
                            cx.notify();
                        }),
                    ),
            )
            .child(
                menu_button("core-resume", true)
                    .flex_1()
                    .child(key_cap("ESC"))
                    .child("Resume")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            view.core_gui.menu = false;
                            cx.notify();
                        }),
                    ),
            );
        backdrop.child(
            panel
                .child(tabs)
                .child(div().flex_1().min_h_0().overflow_hidden().child(page))
                .child(buttons),
        )
    }

    /// The menu's People page: Invite Friends, then everyone in the server.
    fn menu_people(&self) -> Div {
        let players = self
            .core_gui
            .state
            .as_ref()
            .and_then(|s| s.players.clone())
            .unwrap_or_default();
        let toggle = |glyph, on: bool| {
            div()
                .size(px(40.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .when(on, |this| this.bg(rgba(0xffffff1f)))
                .child(icon(glyph, 20.))
        };
        div()
            .px(px(32.))
            .pt(px(18.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .h(px(48.))
                    .px(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(6.))
                    .bg(rgba(0xffffff14))
                    .text_size(px(17.))
                    .child(icon(Glyph::Invite, 22.))
                    .child("Invite Friends"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("In this server"),
                    )
                    .child(
                        div()
                            .p(px(2.))
                            .flex()
                            .rounded(px(8.))
                            .bg(rgba(0xffffff0d))
                            .child(toggle(Glyph::Grid, false))
                            .child(toggle(Glyph::List, true)),
                    ),
            )
            .children(players.into_iter().map(|player| {
                let initial = player.display_name.chars().next().unwrap_or('?');
                div()
                    .h(px(60.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .rounded(px(6.))
                    .bg(rgba(0xffffff0d))
                    .child(
                        div()
                            .size(px(48.))
                            .rounded_full()
                            .bg(rgb(0x3a3d44))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .child(initial.to_uppercase().to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(18.)).child(player.display_name.clone()))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .text_color(rgb(0xc4c6ca))
                                    .child(format!("@{}", player.name)),
                            ),
                    )
                    .child(
                        div()
                            .size(px(40.))
                            .rounded(px(6.))
                            .bg(rgba(0xffffff1f))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(Glyph::Zoom, 24.)),
                    )
            }))
    }

    /// The menu's Settings page: the run's volume.
    fn menu_settings(&self, cx: &mut Context<Self>) -> Div {
        let volume = self.core_gui.volume;
        let segments = (1..=10usize).map(|i| {
            let level = i as f32 / 10.;
            div()
                .id(("core-volume", i))
                .flex_1()
                .h(px(16.))
                .rounded(px(3.))
                .cursor_pointer()
                .bg(if level <= volume + 0.001 {
                    rgb(0xffffff)
                } else {
                    rgba(0xffffff26)
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
        div().px(px(32.)).pt(px(24.)).child(
            div()
                .h(px(56.))
                .px(px(20.))
                .flex()
                .items_center()
                .gap(px(16.))
                .rounded(px(6.))
                .bg(rgba(0xffffff0d))
                .text_size(px(17.))
                .child(div().w(px(140.)).child("Volume"))
                .child(div().flex_1().flex().gap_1().children(segments))
                .child(
                    div()
                        .w(px(44.))
                        .text_right()
                        .child(format!("{:.0}", volume * 10.)),
                ),
        )
    }

    /// The test purchase dialog: Buy or Cancel, never real Robux; the other
    /// prompts (avatar saves, group joins) Allow or Cancel, nothing saved.
    fn purchase_dialog(&self, title: &str, line: &str, cx: &mut Context<Self>) -> Div {
        let confirm = if title == "Test purchase" {
            "Buy (test)"
        } else {
            "Allow (test)"
        };
        let button = |id: &'static str, label: &'static str, bought: bool| {
            menu_button(id, bought)
                .flex_1()
                .h(px(44.))
                .font_weight(FontWeight::NORMAL)
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
            .bg(rgba(0x00000080))
            .child(
                div()
                    .w(px(400.))
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded(px(10.))
                    .bg(rgba(0x191b1df2))
                    .text_color(rgb(WHITE))
                    .text_size(px(15.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .child(title.to_owned()),
                    )
                    .child(div().text_color(rgb(0xc4c6ca)).child(line.to_owned()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(button("core-purchase-cancel", "Cancel", false))
                            .child(button("core-purchase-buy", confirm, true)),
                    ),
            )
    }

    /// The backpack hotbar, bottom centre. A tool with a `TextureId` shows
    /// its image, the others their name; the held one's outline lies over
    /// the slot (and its number) without moving what is inside.
    fn core_hotbar(&self, hotbar: &[HotbarSlot], cx: &mut Context<Self>) -> Div {
        let slots = hotbar.iter().take(10).enumerate().map(|(i, slot)| {
            let key = NUMBER_KEYS[i];
            let icon = self.core_gui.icons.get(&slot.icon).cloned().flatten();
            div()
                .id(("core-hotbar", i))
                .size(px(60.))
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .px(px(1.))
                .rounded(px(2.))
                .bg(rgba(0x191b1f80))
                .text_color(rgb(0xffffff))
                .text_size(px(15.))
                .line_height(px(17.))
                .cursor_pointer()
                .child(match icon {
                    Some(image) => div().size_full().p(px(4.)).child(img(image).size_full()),
                    None => div()
                        .w_full()
                        .text_center()
                        .overflow_hidden()
                        .child(slot.name.clone()),
                })
                .child(
                    div()
                        .absolute()
                        .top(px(1.))
                        .left(px(3.))
                        .text_size(px(15.))
                        .child(format!("{}", (i + 1) % 10)),
                )
                .when(slot.held, |this| {
                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(2.))
                            .border(px(4.))
                            .border_color(rgb(HELD)),
                    )
                })
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
        div()
            .absolute()
            .bottom(px(5.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .gap(px(5.))
            .children(slots)
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
        if client_font(cx) {
            root = root.font_family(FONT);
        }
        if let Some(state) = &core.state {
            let dead = state.health.is_some_and(|(now, _)| now <= 0.0);
            if dead {
                root = root.child(vignette());
            }
            let scale = window.scale_factor();
            for label in &state.labels {
                if let Some(at) = self.project_world(glam::Vec3::from(label.at), scale) {
                    root = root.child(name_label(&label.text, label.health, at));
                }
            }
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
            // The menu hides the hotbar, chat and the People list.
            if !core.menu {
                if let Some(hotbar) = state.hotbar.as_ref().filter(|h| !h.is_empty()) {
                    root = root.child(self.core_hotbar(hotbar, cx));
                }
                if state.chat && core.chat_open {
                    root = root.child(self.core_chat(state.chat_bar, cx));
                }
                if let Some(players) = state.players.as_ref().filter(|_| !core.list_hidden) {
                    root = root.child(player_list(players, &state.stats, cx));
                }
            }
            if let Some((now, max)) = state.health.filter(|(now, max)| now < max) {
                root = root.child(health(share(now, max)));
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
        if let Some(state) = &core.state {
            if core.menu {
                root = root.child(self.core_menu(cx));
            }
            if state.topbar {
                root = root.child(self.core_topbar(state.chat, cx));
            }
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

#[cfg(test)]
mod tests {
    use gpui_kit::Keystroke;
    use rbx_sim::gui::CoreGui;

    use super::{Confirm, CoreAction, CoreGuiView};

    fn press(view: &mut CoreGuiView, key: &str) -> bool {
        let keystroke = Keystroke {
            key: key.into(),
            key_char: (key.chars().count() == 1).then(|| key.into()),
            ..Keystroke::default()
        };
        view.key(&keystroke, true)
    }

    fn playing(state: CoreGui) -> CoreGuiView {
        let mut view = CoreGuiView::default();
        view.update(state);
        view
    }

    fn everything() -> CoreGui {
        CoreGui {
            chat: true,
            chat_bar: true,
            topbar: true,
            can_reset: true,
            ..CoreGui::default()
        }
    }

    #[test]
    fn slash_opens_the_chat_and_enter_says_it() {
        let mut view = playing(everything());
        assert!(!view.chat_open);
        assert!(press(&mut view, "/"));
        assert!(view.chat_open && view.has_keys());
        press(&mut view, "h");
        press(&mut view, "i");
        press(&mut view, "enter");
        assert_eq!(view.take_actions(), [CoreAction::Say("hi".into())]);
        assert!(view.chat_open && !view.has_keys());
    }

    #[test]
    fn a_chat_bar_turned_off_takes_no_typing() {
        let mut view = playing(CoreGui {
            chat_bar: false,
            ..everything()
        });
        assert!(!press(&mut view, "/"));
        assert!(!view.has_keys());
        view.toggle_chat();
        assert!(view.chat_open);
        view.update(CoreGui {
            chat: false,
            ..everything()
        });
        assert!(!view.chat_open);
    }

    #[test]
    fn respawn_asks_first() {
        let mut view = playing(everything());
        press(&mut view, "escape");
        assert!(view.menu_open());
        press(&mut view, "r");
        assert_eq!(view.confirm, Some(Confirm::Respawn));
        press(&mut view, "escape");
        assert!(view.menu_open() && view.confirm.is_none());
        press(&mut view, "r");
        press(&mut view, "enter");
        assert_eq!(view.take_actions(), [CoreAction::Reset]);
        assert!(!view.menu_open());
    }

    #[test]
    fn respawn_off_never_asks() {
        let mut view = playing(CoreGui {
            can_reset: false,
            ..everything()
        });
        press(&mut view, "escape");
        press(&mut view, "r");
        assert!(view.confirm.is_none());
        press(&mut view, "l");
        press(&mut view, "enter");
        assert_eq!(view.take_actions(), [CoreAction::Leave]);
    }

    #[test]
    fn tab_toggles_the_people_list() {
        let mut view = playing(everything());
        assert!(press(&mut view, "tab"));
        assert!(view.list_hidden);
        press(&mut view, "tab");
        assert!(!view.list_hidden);
    }
}
