//! A virtual gamepad the real client sees as hardware: the editor's
//! Controller Emulator.
//!
//! No script API can fabricate gamepad input inside the client, so the pad
//! is made at the OS level instead — a Linux `uinput` device, which the
//! client cannot tell from a plugged-in controller, in the same spirit as
//! the free camera's OS-level mouse capture. Each [`Device`] copies the
//! name, USB ids and evdev layout the matching kernel driver gives the real
//! controller (`xpad` for Xbox, `hid-playstation` for PS4/PS5), so input
//! stacks that key their button maps on those ids (SDL's controller
//! database, Android's per-vendor key layouts) treat it as that pad.
//!
//! Windows and macOS have no equivalent without a driver: Windows would
//! need ViGEmBus (a kernel driver the user installs), macOS a DriverKit
//! HID extension. [`VirtualPad::create`] reports that rather than trying.

// Only the Linux back end reads the layouts and identities; elsewhere they
// stay compiled (and unit-tested) so the model is the same on every platform.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod layout;
#[cfg(target_os = "linux")]
mod uinput;

pub use layout::RawEvent;

/// A device Studio's Controller Emulator offers, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Device {
    Generic,
    Ps4,
    Ps5,
    Xbox,
    Handheld,
    TvRemote,
    Quest,
}

impl Device {
    pub const ALL: [Device; 7] = [
        Device::Generic,
        Device::Ps4,
        Device::Ps5,
        Device::Xbox,
        Device::Handheld,
        Device::TvRemote,
        Device::Quest,
    ];

    /// Studio's own name for it.
    pub fn label(self) -> &'static str {
        match self {
            Device::Generic => "Generic Gamepad",
            Device::Ps4 => "PS4",
            Device::Ps5 => "PS5",
            Device::Xbox => "Xbox",
            Device::Handheld => "Generic Handheld",
            Device::TvRemote => "TV Remote",
            Device::Quest => "Quest2 / Quest3",
        }
    }

    /// A stable name for settings files.
    pub fn key(self) -> &'static str {
        match self {
            Device::Generic => "generic",
            Device::Ps4 => "ps4",
            Device::Ps5 => "ps5",
            Device::Xbox => "xbox",
            Device::Handheld => "handheld",
            Device::TvRemote => "tv_remote",
            Device::Quest => "quest",
        }
    }

    pub fn from_key(key: &str) -> Option<Device> {
        Device::ALL.into_iter().find(|device| device.key() == key)
    }

    /// Why this device can't be emulated here, if it can't.
    pub fn unavailable(self) -> Option<&'static str> {
        match self {
            // Sober has no VR runtime, and a headset's input is a pose
            // stream, not an evdev device.
            Device::Quest => Some("VR needs a headset runtime the Linux client doesn't have"),
            // Roblox maps a remote's D-pad, centre and back keys to gamepad
            // keycodes only on Android TV; the desktop client would read the
            // same evdev keys as plain arrow/Enter/Back keyboard input.
            Device::TvRemote => {
                Some("the desktop client reads a remote's keys as a keyboard, not a TV remote")
            }
            _ => None,
        }
    }

    /// What a control is called on this device, for the drawing and the
    /// mapping editor. The generic pads use Roblox's own `KeyCode` names.
    pub fn button_label(self, button: Button) -> &'static str {
        use Button::*;
        let playstation = matches!(self, Device::Ps4 | Device::Ps5);
        match (button, playstation, self) {
            (South, true, _) => "Cross",
            (East, true, _) => "Circle",
            (West, true, _) => "Square",
            (North, true, _) => "Triangle",
            (LeftBumper, true, _) => "L1",
            (RightBumper, true, _) => "R1",
            (Select, true, Device::Ps4) => "Share",
            (Select, true, _) => "Create",
            (Start, true, _) => "Options",
            (Home, true, _) => "PS",
            (LeftStick, true, _) => "L3",
            (RightStick, true, _) => "R3",
            (South, ..) => "A",
            (East, ..) => "B",
            (West, ..) => "X",
            (North, ..) => "Y",
            (LeftBumper, _, Device::Xbox) => "LB",
            (RightBumper, _, Device::Xbox) => "RB",
            (Select, _, Device::Xbox) => "View",
            (Start, _, Device::Xbox) => "Menu",
            (Home, _, Device::Xbox) => "Guide",
            (LeftStick, _, Device::Xbox) => "LS",
            (RightStick, _, Device::Xbox) => "RS",
            (LeftBumper, ..) => "L1",
            (RightBumper, ..) => "R1",
            (Select, ..) => "Select",
            (Start, ..) => "Start",
            (Home, ..) => "Home",
            (LeftStick, ..) => "L3",
            (RightStick, ..) => "R3",
            (DpadUp, ..) => "D-pad Up",
            (DpadDown, ..) => "D-pad Down",
            (DpadLeft, ..) => "D-pad Left",
            (DpadRight, ..) => "D-pad Right",
        }
    }

    /// The left or right trigger's name on this device.
    pub fn trigger_label(self, side: Side) -> &'static str {
        match (self, side) {
            (Device::Xbox, Side::Left) => "LT",
            (Device::Xbox, Side::Right) => "RT",
            (_, Side::Left) => "L2",
            (_, Side::Right) => "R2",
        }
    }

    /// The evdev key code this device's driver reports for `button`, or
    /// `None` for D-pad directions (a hat axis) and unavailable devices.
    pub fn button_code(self, button: Button) -> Option<u16> {
        layout::layout(self)?.key_code(button)
    }
}

/// A digital control. Positional names: `South` is Xbox A / PlayStation
/// Cross / Roblox `ButtonA`, `East` is `ButtonB`, `West` is `ButtonX`,
/// `North` is `ButtonY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    Select,
    Start,
    Home,
    LeftStick,
    RightStick,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

impl Button {
    pub const ALL: [Button; 15] = [
        Button::South,
        Button::East,
        Button::West,
        Button::North,
        Button::LeftBumper,
        Button::RightBumper,
        Button::Select,
        Button::Start,
        Button::Home,
        Button::LeftStick,
        Button::RightStick,
        Button::DpadUp,
        Button::DpadDown,
        Button::DpadLeft,
        Button::DpadRight,
    ];

    fn bit(self) -> u16 {
        1 << Button::ALL.iter().position(|b| *b == self).unwrap_or(0)
    }
}

/// Which stick or trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

/// Everything the pad reports at one moment. Sticks are `(x, y)` in
/// `-1..=1` with x right and y *up*, as Roblox's `Thumbstick` positions are;
/// triggers are `0..=1`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadState {
    buttons: u16,
    pub sticks: [(f32, f32); 2],
    pub triggers: [f32; 2],
}

impl PadState {
    pub fn pressed(&self, button: Button) -> bool {
        self.buttons & button.bit() != 0
    }

    pub fn set(&mut self, button: Button, down: bool) {
        if down {
            self.buttons |= button.bit();
        } else {
            self.buttons &= !button.bit();
        }
    }

    pub fn stick(&self, side: Side) -> (f32, f32) {
        self.sticks[side as usize]
    }

    pub fn set_stick(&mut self, side: Side, x: f32, y: f32) {
        self.sticks[side as usize] = (x.clamp(-1., 1.), y.clamp(-1., 1.));
    }

    pub fn trigger(&self, side: Side) -> f32 {
        self.triggers[side as usize]
    }

    pub fn set_trigger(&mut self, side: Side, value: f32) {
        self.triggers[side as usize] = value.clamp(0., 1.);
    }
}

/// Why no pad could be made.
#[derive(Debug)]
pub enum PadError {
    /// The device or platform can't be emulated; the reason is for the UI.
    Unavailable(String),
    /// `/dev/uinput` exists but this user can't open it; the text says how
    /// to allow it.
    NoAccess(String),
    Io(std::io::Error),
}

impl std::fmt::Display for PadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PadError::Unavailable(reason) | PadError::NoAccess(reason) => f.write_str(reason),
            PadError::Io(error) => write!(f, "creating the virtual pad failed: {error}"),
        }
    }
}

impl std::error::Error for PadError {}

#[cfg(target_os = "linux")]
pub use uinput::VirtualPad;

/// Off Linux there is no driver-free way to add a gamepad.
#[cfg(not(target_os = "linux"))]
pub struct VirtualPad {
    _never: std::convert::Infallible,
}

#[cfg(not(target_os = "linux"))]
impl VirtualPad {
    pub fn create(_device: Device) -> Result<VirtualPad, PadError> {
        Err(PadError::Unavailable(if cfg!(windows) {
            "Windows needs the ViGEmBus driver installed to add a virtual gamepad".into()
        } else {
            "this platform needs a HID driver extension to add a virtual gamepad".into()
        }))
    }

    pub fn device(&self) -> Device {
        match self._never {}
    }

    pub fn send(&mut self, _next: PadState) -> std::io::Result<()> {
        match self._never {}
    }
}
