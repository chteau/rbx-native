//! Keyboard → pad mappings, one set per device, and the emulator's saved
//! choices in `<config>/controller_emulator.json`.
//!
//! Defaults, the same on every pad (positional, so `Space` is Xbox A and
//! PlayStation Cross alike):
//!
//! | Keys | Control |
//! |---|---|
//! | W A S D | left stick |
//! | I J K L | right stick |
//! | arrow keys | D-pad |
//! | Space / F / R / T | south / east / west / north face button |
//! | Q / E | left / right bumper |
//! | Z / C | left / right trigger (fully pulled) |
//! | X / M | left / right stick click |
//! | Backspace / Enter / Home | select / start / home |
//!
//! WASD and IJKL keep both hands on home-row clusters, as the two thumbs
//! sit on a pad; the arrows stay free for the D-pad, which games use for
//! menus.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rbx_play::gamepad::{Button, Device, PadState, Side};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// What one key drives.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Control {
    Button(Button),
    Stick(Side, Direction),
    Trigger(Side),
}

const DIRECTIONS: [(Direction, &str); 4] = [
    (Direction::Up, "up"),
    (Direction::Down, "down"),
    (Direction::Left, "left"),
    (Direction::Right, "right"),
];

const SIDES: [(Side, &str); 2] = [(Side::Left, "left"), (Side::Right, "right")];

const BUTTON_IDS: [&str; 15] = [
    "south",
    "east",
    "west",
    "north",
    "bumper_left",
    "bumper_right",
    "select",
    "start",
    "home",
    "stick_left",
    "stick_right",
    "dpad_up",
    "dpad_down",
    "dpad_left",
    "dpad_right",
];

impl Control {
    /// Every control, in the mapping editor's order.
    pub(super) fn all() -> Vec<Control> {
        let mut all: Vec<Control> = Button::ALL.into_iter().map(Control::Button).collect();
        for (side, _) in SIDES {
            all.extend(DIRECTIONS.map(|(direction, _)| Control::Stick(side, direction)));
            all.push(Control::Trigger(side));
        }
        all
    }

    /// The stable name the settings file uses.
    fn id(self) -> String {
        let side_id = |side| {
            SIDES
                .iter()
                .find(|(s, _)| *s == side)
                .map_or("", |(_, id)| id)
        };
        match self {
            Control::Button(button) => {
                let index = Button::ALL.iter().position(|b| *b == button).unwrap_or(0);
                format!("button.{}", BUTTON_IDS[index])
            }
            Control::Stick(side, direction) => {
                let direction = DIRECTIONS
                    .iter()
                    .find(|(d, _)| *d == direction)
                    .map_or("", |(_, id)| id);
                format!("stick.{}.{direction}", side_id(side))
            }
            Control::Trigger(side) => format!("trigger.{}", side_id(side)),
        }
    }

    fn from_id(id: &str) -> Option<Control> {
        Control::all()
            .into_iter()
            .find(|control| control.id() == id)
    }

    /// What the mapping editor calls it on `device`.
    pub(super) fn label(self, device: Device) -> String {
        let side = |side| if side == Side::Left { "Left" } else { "Right" };
        match self {
            Control::Button(button) => device.button_label(button).into(),
            Control::Stick(s, direction) => format!("{} stick {direction:?}", side(s)),
            Control::Trigger(s) => device.trigger_label(s).into(),
        }
    }
}

/// Each control's key (gpui's `Keystroke::key` names). One key per
/// control, and a key drives at most one control.
pub(super) type Bindings = HashMap<Control, String>;

pub(super) fn defaults() -> Bindings {
    use Button::*;
    let stick = |side, keys: [&'static str; 4]| {
        DIRECTIONS
            .map(|(direction, _)| Control::Stick(side, direction))
            .into_iter()
            .zip(keys)
    };
    let buttons = [
        (South, "space"),
        (East, "f"),
        (West, "r"),
        (North, "t"),
        (LeftBumper, "q"),
        (RightBumper, "e"),
        (Select, "backspace"),
        (Start, "enter"),
        (Home, "home"),
        (LeftStick, "x"),
        (RightStick, "m"),
        (DpadUp, "up"),
        (DpadDown, "down"),
        (DpadLeft, "left"),
        (DpadRight, "right"),
    ]
    .map(|(button, key)| (Control::Button(button), key));
    buttons
        .into_iter()
        .chain(stick(Side::Left, ["w", "s", "a", "d"]))
        .chain(stick(Side::Right, ["i", "k", "j", "l"]))
        .chain([
            (Control::Trigger(Side::Left), "z"),
            (Control::Trigger(Side::Right), "c"),
        ])
        .map(|(control, key)| (control, key.to_owned()))
        .collect()
}

/// How a key name reads on a chip: "W", "Space", "↑".
pub(super) fn key_label(key: &str) -> String {
    match key {
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        _ if key.chars().count() == 1 => key.to_uppercase(),
        _ => {
            let mut chars = key.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

/// Binds `key` to `control`, replacing the control's old key; returns the
/// control that lost `key`, so the editor can warn about it, as Studio does.
pub(super) fn bind(bindings: &mut Bindings, control: Control, key: &str) -> Option<Control> {
    let displaced = bindings
        .iter()
        .find(|(other, bound)| **other != control && bound.as_str() == key)
        .map(|(other, _)| *other);
    if let Some(other) = displaced {
        bindings.remove(&other);
    }
    bindings.insert(control, key.to_owned());
    displaced
}

/// The pad state the held keys describe. Opposite stick directions cancel,
/// as a real stick can't be both ways at once.
pub(super) fn state_for(held: &HashSet<String>, bindings: &Bindings) -> PadState {
    let mut state = PadState::default();
    let mut sticks = [(0f32, 0f32); 2];
    for (control, key) in bindings {
        if !held.contains(key) {
            continue;
        }
        match *control {
            Control::Button(button) => state.set(button, true),
            Control::Trigger(side) => state.set_trigger(side, 1.),
            Control::Stick(side, direction) => {
                let stick = &mut sticks[side as usize];
                match direction {
                    Direction::Up => stick.1 += 1.,
                    Direction::Down => stick.1 -= 1.,
                    Direction::Left => stick.0 -= 1.,
                    Direction::Right => stick.0 += 1.,
                }
            }
        }
    }
    for (side, (x, y)) in [Side::Left, Side::Right].into_iter().zip(sticks) {
        state.set_stick(side, x, y);
    }
    state
}

/// The pointer's state and the keyboard's, together: buttons from either,
/// a stick from the keys while any of its keys is held, the harder pull of
/// each trigger.
pub(super) fn merge(pointer: &PadState, keys: &PadState) -> PadState {
    let mut state = *pointer;
    for button in Button::ALL {
        if keys.pressed(button) {
            state.set(button, true);
        }
    }
    for side in [Side::Left, Side::Right] {
        let (x, y) = keys.stick(side);
        if x != 0. || y != 0. {
            state.set_stick(side, x, y);
        }
        state.set_trigger(side, pointer.trigger(side).max(keys.trigger(side)));
    }
    state
}

/// What the emulator remembers between sessions.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Saved {
    /// Prepare every Play's client for gamepads, panel open or not.
    pub(super) enabled: bool,
    pub(super) device: Device,
    mappings: HashMap<Device, Bindings>,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            enabled: false,
            device: Device::Xbox,
            mappings: HashMap::new(),
        }
    }
}

impl Saved {
    pub(super) fn bindings(&self, device: Device) -> Bindings {
        self.mappings.get(&device).cloned().unwrap_or_else(defaults)
    }

    pub(super) fn set_bindings(&mut self, device: Device, bindings: Bindings) {
        self.mappings.insert(device, bindings);
    }

    /// Anything unreadable falls back to the defaults, key by key.
    fn from_json(text: &str) -> Saved {
        let mut saved = Saved::default();
        let Ok(serde_json::Value::Object(root)) = serde_json::from_str(text) else {
            return saved;
        };
        saved.enabled = root.get("enabled").and_then(|v| v.as_bool()) == Some(true);
        if let Some(device) = root
            .get("device")
            .and_then(|v| v.as_str())
            .and_then(Device::from_key)
        {
            saved.device = device;
        }
        let Some(serde_json::Value::Object(mappings)) = root.get("mappings") else {
            return saved;
        };
        for (device, keys) in mappings {
            let (Some(device), serde_json::Value::Object(keys)) = (Device::from_key(device), keys)
            else {
                continue;
            };
            let bindings = keys
                .iter()
                .filter_map(|(id, key)| Some((Control::from_id(id)?, key.as_str()?.to_owned())))
                .collect();
            saved.mappings.insert(device, bindings);
        }
        saved
    }

    fn to_json(&self) -> String {
        let mappings: serde_json::Map<String, serde_json::Value> = self
            .mappings
            .iter()
            .map(|(device, bindings)| {
                let keys: serde_json::Map<String, serde_json::Value> = bindings
                    .iter()
                    .map(|(control, key)| (control.id(), key.clone().into()))
                    .collect();
                (device.key().to_owned(), keys.into())
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({
            "enabled": self.enabled,
            "device": self.device.key(),
            "mappings": mappings,
        }))
        .unwrap_or_default()
    }

    pub(super) fn load() -> Saved {
        path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| Saved::from_json(&text))
            .unwrap_or_default()
    }

    pub(super) fn save(&self) {
        if let Some(path) = path() {
            save_to(self, &path);
        }
    }
}

fn save_to(saved: &Saved, path: &Path) {
    // A failed write only loses this session's choices; nothing to report.
    let _ = crate::settings::write_atomic(path, saved.to_json().as_bytes());
}

fn path() -> Option<PathBuf> {
    crate::settings::default_config_dir().map(|dir| dir.join("controller_emulator.json"))
}

#[cfg(test)]
mod tests;
