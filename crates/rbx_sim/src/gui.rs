//! GUI at run time, our own from the docs and the GuiRuntime and
//! CoreGuiApi goldens: StarterGui copied into PlayerGui on spawn (see
//! `players.rs`), a client's `ScreenGui`s laid out (`AbsolutePosition`,
//! `AbsoluteSize`), the mouse and keyboard handed to GuiButtons and
//! TextBoxes before the game sees them, StarterGui's core GUI switches,
//! `SetCore`/`GetCore`, PlayerGui's top bar and GuiService basics.
//!
//! What a click on a button fires (GuiRuntime `log`): on the press
//! `MouseButton1Down`, the button's `InputBegan`, then UserInputService's
//! `InputBegan` with gameProcessedEvent true; on the release
//! `MouseButton1Click`, `MouseButton1Up`, `Activated(input, 0)`, the
//! button's `InputEnded`, then UserInputService's. A TextBox takes focus
//! on the press (its text cleared first with `ClearTextOnFocus`, then
//! `Focused`, `TextBoxFocused`); a key typed into it goes to
//! UserInputService first (processed), then into `Text`; Return lets go
//! (`FocusLost(true, input)`, `TextBoxFocusReleased`) before its own
//! `InputBegan`, so its `InputEnded` isn't processed.

mod layout;

use std::collections::{HashMap, HashSet};

use rbx_dom::{Ref, Variant, Vector2Data, WeakDom};
use rbx_lua::server::{Arg, CallReply, Event};
use rbx_reflection::ReflectionDatabase;

pub(crate) use layout::{Laid, TOPBAR_INSET};

use crate::input::Input;

/// The core GUI kinds `SetCoreGuiEnabled` switches (`Enum.CoreGuiType`
/// less `All`).
const CORE_TYPES: &[&str] = &[
    "PlayerList",
    "Health",
    "Backpack",
    "Chat",
    "EmotesMenu",
    "SelfView",
    "Captures",
    "AvatarSwitcher",
    "ExperienceShop",
];

/// The `SetCore` prompts Local Run has no friends or block list for: the
/// toast each gives instead.
const PROMPT_NOTICES: &[(&str, &str)] = &[
    (
        "PromptSendFriendRequest",
        "Local Run has no friends to add.",
    ),
    ("PromptUnfriend", "Local Run has no friends to remove."),
    ("PromptBlockPlayer", "Local Run has no block list."),
    ("PromptUnblockPlayer", "Local Run has no block list."),
];

/// What `SetCore`/`GetCore` know, with what `GetCore` gives before any
/// `SetCore` (CoreGuiApi: `TopbarEnabled` true; `ResetButtonCallback`
/// reads back what was set). Anything else is "not registered".
const CORE_VALUES: &[(&str, Option<bool>)] = &[
    ("TopbarEnabled", Some(true)),
    ("ResetButtonCallback", Some(true)),
    ("PointsNotificationsActive", Some(true)),
    ("BadgesNotificationsActive", Some(true)),
    ("ChatActive", Some(true)),
    ("ChatBarDisabled", Some(false)),
    ("DevConsoleVisible", Some(false)),
    ("AvatarContextMenuEnabled", Some(false)),
    ("SendNotification", None),
    ("ChatMakeSystemMessage", None),
    ("ChatWindowPosition", None),
    ("ChatWindowSize", None),
    ("PromptSendFriendRequest", None),
    ("PromptUnfriend", None),
    ("PromptBlockPlayer", None),
    ("PromptUnblockPlayer", None),
    ("SetAvatarContextMenuEnabled", None),
    ("AddAvatarContextMenuOption", None),
    ("RemoveAvatarContextMenuOption", None),
    ("AvatarContextMenuTarget", None),
    ("AvatarContextMenuTheme", None),
    ("CoreGuiChatConnections", None),
    ("EmotesMenuOpen", Some(false)),
];

/// `PlayerGui:GetTopbarTransparency()` before any set (CoreGuiApi).
const TOPBAR_TRANSPARENCY: f64 = 0.5;

const GUI_OBJECT_SIGNALS: &[&str] = &[
    "MouseEnter",
    "MouseLeave",
    "MouseMoved",
    "MouseWheelForward",
    "MouseWheelBackward",
    "InputBegan",
    "InputChanged",
    "InputEnded",
    "SelectionGained",
    "SelectionLost",
    "TouchTap",
];
const GUI_BUTTON_SIGNALS: &[&str] = &[
    "MouseButton1Down",
    "MouseButton1Up",
    "MouseButton1Click",
    "MouseButton2Down",
    "MouseButton2Up",
    "MouseButton2Click",
    "Activated",
];

/// What our default core GUI shows for the local player, for the editor
/// to draw (our own look, nothing of Roblox's): each part only while its
/// `CoreGuiType` is on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoreGui {
    /// The character's health and maximum, `Health` on.
    pub health: Option<(f64, f64)>,
    /// The hotbar's tools (name, held), `Backpack` on.
    pub hotbar: Option<Vec<(String, bool)>>,
    /// Everyone in the game, `PlayerList` on.
    pub players: Option<Vec<PlayerRow>>,
    /// The `leaderstats` columns, in the order the first player who has
    /// them holds them.
    pub stats: Vec<String>,
    /// `SendNotification`s not yet shown: (title, text).
    pub notifications: Vec<(String, String)>,
    /// Whether the menu's Reset button works (`ResetButtonCallback` not
    /// `false`).
    pub can_reset: bool,
    /// Each player's head, for chat bubbles: (user id, world position).
    pub heads: Vec<(i64, [f32; 3])>,
}

/// One player in the list: name, team colour, and a value per `stats`
/// column (empty where they have none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerRow {
    pub name: String,
    pub team: Option<[u8; 3]>,
    pub stats: Vec<String>,
}

/// One client's GUI state.
pub struct Gui {
    /// The window in pixels (`Camera.ViewportSize`).
    pub viewport: [f32; 2],
    /// The device's safe-area insets (left, top, right, bottom): a phone's
    /// notch and home bar.
    pub safe_insets: [f32; 4],
    pub(crate) laid: Vec<Laid>,
    mouse: [f32; 2],
    hover: Vec<Ref>,
    /// The GuiButton each mouse button went down on.
    pressed: [Option<Ref>; 2],
    pub(crate) focus: Option<Ref>,
    shift: [bool; 2],
    disabled: HashSet<&'static str>,
    core: HashMap<String, Arg>,
    topbar: f64,
    /// `SendNotification`s, for the editor to show: (title, text).
    pub notifications: Vec<(String, String)>,
    /// `ChatMakeSystemMessage`s, for the chat window.
    pub system_messages: Vec<String>,
}

impl Default for Gui {
    fn default() -> Self {
        Gui {
            // The window conformance runs at (GuiRuntime `_screen`).
            viewport: [800.0, 600.0],
            safe_insets: [0.0; 4],
            laid: Vec::new(),
            mouse: [0.0; 2],
            hover: Vec::new(),
            pressed: [None; 2],
            focus: None,
            shift: [false; 2],
            disabled: HashSet::new(),
            core: HashMap::new(),
            topbar: TOPBAR_TRANSPARENCY,
            notifications: Vec::new(),
            system_messages: Vec::new(),
        }
    }
}

/// Something the client does for the GUI, in order.
pub(crate) enum Step {
    Fire(Event),
    /// A TextBox's text (written, then its change events fire).
    Text(Ref, String),
}

/// What the GUI did with one input: steps before UserInputService hears
/// of it, whether it was processed, steps after.
#[derive(Default)]
pub(crate) struct Handled {
    pub before: Vec<Step>,
    pub processed: bool,
    pub after: Vec<Step>,
}

fn is(db: &ReflectionDatabase, dom: &WeakDom, r: Ref, class: &str) -> bool {
    dom.get(r)
        .is_some_and(|i| db.is_subclass_of(i.class(), class))
}

fn signal(owner: Ref, name: &str, args: Vec<Arg>) -> Step {
    Step::Fire(Event::Signal {
        owner,
        name: name.into(),
        args,
    })
}

fn text_of(dom: &WeakDom, r: Ref) -> String {
    match dom.get(r).and_then(|i| i.properties().get("Text")) {
        Some(Variant::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// The character a key types, US layout, `None` for keys that type none.
fn typed(code: &str, shift: bool) -> Option<char> {
    if code.len() == 1 {
        let c = code.chars().next()?;
        if c.is_ascii_alphabetic() {
            return Some(if shift {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            });
        }
    }
    const DIGITS: [&str; 10] = [
        "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
    ];
    if let Some(n) = DIGITS.iter().position(|d| *d == code) {
        return Some(if shift {
            b")!@#$%^&*("[n] as char
        } else {
            (b'0' + n as u8) as char
        });
    }
    let (plain, shifted) = match code {
        "Space" => (' ', ' '),
        "Minus" => ('-', '_'),
        "Equals" => ('=', '+'),
        "LeftBracket" => ('[', '{'),
        "RightBracket" => (']', '}'),
        "BackSlash" => ('\\', '|'),
        "Semicolon" => (';', ':'),
        "Quote" => ('\'', '"'),
        "Comma" => (',', '<'),
        "Period" => ('.', '>'),
        "Slash" => ('/', '?'),
        "Backquote" => ('`', '~'),
        _ => return None,
    };
    Some(if shift { shifted } else { plain })
}

impl Gui {
    /// Whether the core GUI of `kind` (a `CoreGuiType` name) is on.
    /// What `SetCore("ResetButtonCallback", …)` was given, if anything.
    pub(crate) fn reset_callback(&self) -> Option<&Arg> {
        self.core.get("ResetButtonCallback")
    }

    pub fn core_enabled(&self, kind: &str) -> bool {
        match kind {
            "All" => CORE_TYPES.iter().all(|k| !self.disabled.contains(k)),
            _ => !self.disabled.contains(kind),
        }
    }

    /// Lays out `root` (the local `PlayerGui`) and writes each object's
    /// `AbsolutePosition`, `AbsoluteSize` and `AbsoluteRotation` where they
    /// changed (counted from below the top bar inset, as Roblox's are
    /// whether or not a screen ignores it: GuiRuntime `button_pos`).
    pub(crate) fn relayout(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, root: Ref) {
        self.laid = layout::layout(dom, db, root, self.viewport, self.safe_insets);
        for laid in &self.laid {
            let v2 = |x: f32, y: f32| Variant::Vector2(Vector2Data { x, y });
            let mut values = vec![
                (
                    "AbsolutePosition",
                    v2(laid.rect[0], laid.rect[1] - TOPBAR_INSET),
                ),
                ("AbsoluteSize", v2(laid.rect[2], laid.rect[3])),
            ];
            if !laid.screen {
                values.push(("AbsoluteRotation", Variant::Float32(laid.rotation)));
            }
            for (name, value) in values {
                let same = dom
                    .get(laid.referent)
                    .is_some_and(|i| i.properties().get(name) == Some(&value));
                if !same {
                    let _ = dom.set_property(laid.referent, name, value);
                }
            }
        }
        if self
            .focus
            .is_some_and(|f| !self.laid.iter().any(|l| l.referent == f && l.shown))
        {
            self.focus = None;
        }
    }

    fn at(&self) -> Vec<Arg> {
        vec![
            Arg::Number(f64::from(self.mouse[0])),
            Arg::Number(f64::from(self.mouse[1] - TOPBAR_INSET)),
        ]
    }

    /// The object a press lands on: the topmost one that takes input (a
    /// GuiButton, a TextBox, or `Active`).
    fn target(&self, dom: &WeakDom, db: &ReflectionDatabase) -> Option<Ref> {
        layout::hits(&self.laid, self.mouse).into_iter().find(|&r| {
            is(db, dom, r, "GuiButton")
                || is(db, dom, r, "TextBox")
                || layout::flag(dom, db, r, "Active")
        })
    }

    fn release_focus(&mut self, enter: bool, input: Arg, uis: Ref, steps: &mut Vec<Step>) {
        if let Some(box_) = self.focus.take() {
            steps.push(signal(box_, "FocusLost", vec![Arg::Bool(enter), input]));
            steps.push(signal(
                uis,
                "TextBoxFocusReleased",
                vec![Arg::Instance(box_)],
            ));
        }
    }

    fn take_focus(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        box_: Ref,
        uis: Ref,
        steps: &mut Vec<Step>,
    ) {
        if layout::flag(dom, db, box_, "ClearTextOnFocus") {
            steps.push(Step::Text(box_, String::new()));
        }
        self.focus = Some(box_);
        steps.push(signal(box_, "Focused", Vec::new()));
        steps.push(signal(uis, "TextBoxFocused", vec![Arg::Instance(box_)]));
    }

    /// One input the engine wrote into `object`: what the GUI fires and
    /// whether that took it from the game.
    pub(crate) fn input(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        input: &Input,
        object: Ref,
        uis: Ref,
    ) -> Handled {
        let mut out = Handled::default();
        let me = Arg::Instance(object);
        match input {
            Input::Move { position, .. } => {
                self.mouse = *position;
                let hits = layout::hits(&self.laid, self.mouse);
                for &gone in self.hover.iter().filter(|r| !hits.contains(r)) {
                    out.before.push(signal(gone, "MouseLeave", self.at()));
                }
                for &r in &hits {
                    let name = if self.hover.contains(&r) {
                        "MouseMoved"
                    } else {
                        "MouseEnter"
                    };
                    out.before.push(signal(r, name, self.at()));
                }
                self.hover = hits;
                out.processed = self.target(dom, db).is_some();
            }
            Input::Button { button, down } if *button == 1 || *button == 2 => {
                let slot = usize::from(*button == 2);
                let n = if *button == 2 { 2 } else { 1 };
                let target = self.target(dom, db);
                if *down {
                    if *button == 1 && target != self.focus {
                        self.release_focus(false, me.clone(), uis, &mut out.before);
                    }
                    match target {
                        Some(t) if is(db, dom, t, "TextBox") => {
                            if *button == 1 && self.focus != Some(t) {
                                self.take_focus(dom, db, t, uis, &mut out.before);
                            }
                        }
                        Some(t) if is(db, dom, t, "GuiButton") => {
                            self.pressed[slot] = Some(t);
                            out.before
                                .push(signal(t, &format!("MouseButton{n}Down"), self.at()));
                            out.before.push(signal(t, "InputBegan", vec![me.clone()]));
                        }
                        Some(t) => out.before.push(signal(t, "InputBegan", vec![me.clone()])),
                        None => {}
                    }
                    out.processed = target.is_some();
                } else if let Some(t) = self.pressed[slot].take() {
                    if target == Some(t) {
                        out.before
                            .push(signal(t, &format!("MouseButton{n}Click"), Vec::new()));
                        out.before
                            .push(signal(t, &format!("MouseButton{n}Up"), self.at()));
                        if n == 1 {
                            out.before.push(signal(
                                t,
                                "Activated",
                                vec![me.clone(), Arg::Number(0.0)],
                            ));
                        }
                    }
                    out.before.push(signal(t, "InputEnded", vec![me]));
                    out.processed = true;
                } else {
                    if let Some(t) = target {
                        out.before.push(signal(t, "InputEnded", vec![me]));
                    }
                    out.processed = target.is_some();
                }
            }
            Input::Wheel { notches } => {
                if let Some(&top) = layout::hits(&self.laid, self.mouse).first() {
                    let name = if *notches > 0.0 {
                        "MouseWheelForward"
                    } else {
                        "MouseWheelBackward"
                    };
                    out.before.push(signal(top, name, self.at()));
                }
                out.processed = self.target(dom, db).is_some();
            }
            Input::Key { code, down } => {
                match code.as_str() {
                    "LeftShift" => self.shift[0] = *down,
                    "RightShift" => self.shift[1] = *down,
                    _ => {}
                }
                let Some(box_) = self.focus else {
                    return out;
                };
                out.processed = true;
                if !*down {
                    return out;
                }
                let multi = layout::flag(dom, db, box_, "MultiLine");
                let editable = layout::flag(dom, db, box_, "TextEditable");
                let text = text_of(dom, box_);
                match code.as_str() {
                    "Return" | "KeypadEnter" if !multi => {
                        self.release_focus(true, me, uis, &mut out.before);
                    }
                    "Escape" => self.release_focus(false, me, uis, &mut out.before),
                    "Backspace" if editable => {
                        let mut text = text;
                        if text.pop().is_some() {
                            out.after.push(Step::Text(box_, text));
                        }
                    }
                    "Return" | "KeypadEnter" if editable => {
                        out.after.push(Step::Text(box_, text + "\n"));
                    }
                    _ if editable => {
                        if let Some(c) = typed(code, self.shift[0] || self.shift[1]) {
                            out.after.push(Step::Text(box_, format!("{text}{c}")));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        out
    }
}

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    let db = ReflectionDatabase::shared();
    match class {
        "StarterGui" => matches!(
            key,
            "SetCoreGuiEnabled" | "GetCoreGuiEnabled" | "SetCore" | "GetCore"
        ),
        "PlayerGui" => matches!(key, "GetTopbarTransparency" | "SetTopbarTransparency"),
        "GuiService" => matches!(key, "GetGuiInset" | "IsTenFootInterface"),
        "UserInputService" => key == "GetFocusedTextBox",
        _ => {
            db.is_subclass_of(class, "TextBox")
                && matches!(key, "CaptureFocus" | "ReleaseFocus" | "IsFocused")
        }
    }
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    let db = ReflectionDatabase::shared();
    (db.is_subclass_of(class, "GuiObject") && GUI_OBJECT_SIGNALS.contains(&key))
        || (db.is_subclass_of(class, "GuiButton") && GUI_BUTTON_SIGNALS.contains(&key))
        || (db.is_subclass_of(class, "TextBox") && matches!(key, "Focused" | "FocusLost"))
        || (class == "StarterGui" && key == "CoreGuiChangedSignal")
        || (class == "PlayerGui" && key == "TopbarTransparencyChangedSignal")
        || (class == "UserInputService" && matches!(key, "TextBoxFocused" | "TextBoxFocusReleased"))
}

/// The `CoreGuiType` an argument names (an enum item or its name).
fn core_type(arg: Option<&Arg>) -> Result<String, String> {
    match arg {
        Some(Arg::Enum { item, .. }) => Ok(item.clone()),
        Some(Arg::String(name)) if name == "All" || CORE_TYPES.contains(&name.as_str()) => {
            Ok(name.clone())
        }
        _ => Err("Argument 1 missing or nil".into()),
    }
}

fn uis_of(dom: &WeakDom) -> Option<Ref> {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "UserInputService"))
}

/// A GUI engine method a script called.
pub(crate) fn call(
    gui: &mut Gui,
    client: bool,
    dom: &mut WeakDom,
    referent: Ref,
    class: &str,
    method: &str,
    args: &[Arg],
) -> Result<CallReply, String> {
    let fire = |fire: Vec<(Ref, String, Vec<Arg>)>| CallReply {
        fire,
        ..CallReply::default()
    };
    match method {
        "GetCoreGuiEnabled" => {
            let kind = core_type(args.first())?;
            Ok(CallReply::value(Arg::Bool(gui.core_enabled(&kind))))
        }
        "SetCoreGuiEnabled" => {
            let kind = core_type(args.first())?;
            let on = !matches!(args.get(1), Some(Arg::Bool(false)) | Some(Arg::Nil) | None);
            let kinds: Vec<&'static str> = match CORE_TYPES.iter().find(|k| **k == kind) {
                Some(k) => vec![*k],
                None => CORE_TYPES.to_vec(),
            };
            for k in kinds {
                if on {
                    gui.disabled.remove(k);
                } else {
                    gui.disabled.insert(k);
                }
            }
            let item = Arg::Enum {
                enum_name: "CoreGuiType".into(),
                item: kind,
            };
            Ok(fire(vec![(
                referent,
                "CoreGuiChangedSignal".into(),
                vec![item, Arg::Bool(on)],
            )]))
        }
        "SetCore" | "GetCore" => {
            let Some(Arg::String(name)) = args.first() else {
                return Err("Argument 1 missing or nil".into());
            };
            let known = CORE_VALUES.iter().find(|(n, _)| n == name);
            // The server never registers them: SetCore does nothing there
            // (and its GetCore never gets here: it waits forever, see
            // `rbx_lua::server`'s hooks).
            if !client {
                return Ok(CallReply::default());
            }
            let Some((_, default)) = known else {
                return Err(format!(
                    "{method}: {name} has not been registered by the CoreScripts"
                ));
            };
            if method == "SetCore" {
                let value = args.get(1).cloned().unwrap_or(Arg::Nil);
                let field = |key: &str| match &value {
                    Arg::Table(fields) => fields.iter().find_map(|(k, v)| match v {
                        Arg::String(s) if k == key => Some(s.clone()),
                        _ => None,
                    }),
                    _ => None,
                };
                match name.as_str() {
                    "SendNotification" if matches!(value, Arg::Table(_)) => {
                        gui.notifications.push((
                            field("Title").unwrap_or_default(),
                            field("Text").unwrap_or_default(),
                        ));
                    }
                    "ChatMakeSystemMessage" => {
                        if let Some(text) = field("Text") {
                            gui.system_messages.push(text);
                        }
                    }
                    _ => {
                        if let Some((_, text)) = PROMPT_NOTICES.iter().find(|(n, _)| n == name) {
                            gui.notifications.push((name.clone(), (*text).to_owned()));
                        }
                    }
                }
                gui.core.insert(name.clone(), value);
                return Ok(CallReply::default());
            }
            let value = gui
                .core
                .get(name)
                .cloned()
                .or(default.map(Arg::Bool))
                .unwrap_or(Arg::Nil);
            Ok(CallReply::value(value))
        }
        "GetTopbarTransparency" => Ok(CallReply::value(Arg::Number(gui.topbar))),
        "SetTopbarTransparency" => {
            let Some(Arg::Number(x)) = args.first() else {
                return Err("Argument 1 missing or nil".into());
            };
            gui.topbar = *x;
            Ok(fire(vec![(
                referent,
                "TopbarTransparencyChangedSignal".into(),
                vec![Arg::Number(*x)],
            )]))
        }
        "GetGuiInset" => Ok(CallReply {
            values: vec![Arg::Vector2([0.0, TOPBAR_INSET]), Arg::Vector2([0.0, 0.0])],
            ..CallReply::default()
        }),
        "IsTenFootInterface" => Ok(CallReply::value(Arg::Bool(false))),
        "GetFocusedTextBox" => Ok(CallReply::value(gui.focus.map_or(Arg::Nil, Arg::Instance))),
        "IsFocused" => Ok(CallReply::value(Arg::Bool(gui.focus == Some(referent)))),
        "CaptureFocus" | "ReleaseFocus" => {
            let mut steps = Vec::new();
            let uis = uis_of(dom).unwrap_or(referent);
            let db = ReflectionDatabase::shared();
            if method == "CaptureFocus" && client && gui.focus != Some(referent) {
                gui.release_focus(false, Arg::Nil, uis, &mut steps);
                gui.take_focus(dom, db, referent, uis, &mut steps);
            } else if method == "ReleaseFocus" && gui.focus == Some(referent) {
                let submitted = matches!(args.first(), Some(Arg::Bool(true)));
                gui.release_focus(submitted, Arg::Nil, uis, &mut steps);
            }
            let mut out = Vec::new();
            for step in steps {
                match step {
                    Step::Fire(Event::Signal { owner, name, args }) => {
                        out.push((owner, name, args))
                    }
                    Step::Text(r, text) => {
                        let _ = dom.set_property(r, "Text", Variant::String(text));
                    }
                    Step::Fire(_) => {}
                }
            }
            let _ = class;
            Ok(fire(out))
        }
        _ => Err(format!("{method} is not a valid member of {class}")),
    }
}
