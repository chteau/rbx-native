//! Each device's evdev layout, and the events a state change becomes.
//!
//! Codes and ranges are the kernel drivers' own (`drivers/input/joystick/
//! xpad.c`, `drivers/hid/hid-playstation.c`), so the pad reports exactly
//! what the real controller would through them. The generic pads follow
//! `Documentation/input/gamepad.rst` positionally, with triggers on
//! `ABS_Z`/`ABS_RZ` as every common driver does (the document's
//! `ABS_HAT2*` trigger axes are not what userspace mappers expect).

use super::{Button, Device, PadState, Side};

pub(super) const EV_SYN: u16 = 0x00;
pub(super) const EV_KEY: u16 = 0x01;
pub(super) const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0;

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_Z: u16 = 0x02;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;
const ABS_RZ: u16 = 0x05;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;

const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
// `BTN_X` and `BTN_Y` are aliases of these two, which is why `xpad`
// reporting X as `BTN_X` lands on `BTN_NORTH` though X is the west button.
const BTN_NORTH: u16 = 0x133;
const BTN_WEST: u16 = 0x134;
const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_TL2: u16 = 0x138;
const BTN_TR2: u16 = 0x139;
const BTN_SELECT: u16 = 0x13a;
const BTN_START: u16 = 0x13b;
const BTN_MODE: u16 = 0x13c;
const BTN_THUMBL: u16 = 0x13d;
const BTN_THUMBR: u16 = 0x13e;

/// One `struct input_event`, without its timestamp (the kernel stamps it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawEvent {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

/// An absolute axis' range, as `input_set_abs_params` takes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Range {
    pub(super) min: i32,
    pub(super) max: i32,
    pub(super) fuzz: i32,
    pub(super) flat: i32,
}

/// What the driver calls itself and which USB ids it carries.
#[derive(Clone, Copy, Debug)]
pub(super) struct Identity {
    pub(super) name: &'static str,
    pub(super) vendor: u16,
    pub(super) product: u16,
    pub(super) version: u16,
}

pub(super) struct Layout {
    pub(super) identity: Identity,
    buttons: [(Button, u16); 11],
    stick: Range,
    trigger: Range,
    /// `hid-playstation` also reports L2/R2 as keys while pulled.
    trigger_keys: bool,
}

const HAT: Range = Range {
    min: -1,
    max: 1,
    fuzz: 0,
    flat: 0,
};

const XPAD_STICK: Range = Range {
    min: -32768,
    max: 32767,
    fuzz: 16,
    flat: 128,
};

const BYTE: Range = Range {
    min: 0,
    max: 255,
    fuzz: 0,
    flat: 0,
};

/// `gamepad.rst`'s positional order, which `hid-playstation` follows.
const POSITIONAL: [(Button, u16); 11] = [
    (Button::South, BTN_SOUTH),
    (Button::East, BTN_EAST),
    (Button::West, BTN_WEST),
    (Button::North, BTN_NORTH),
    (Button::LeftBumper, BTN_TL),
    (Button::RightBumper, BTN_TR),
    (Button::Select, BTN_SELECT),
    (Button::Start, BTN_START),
    (Button::Home, BTN_MODE),
    (Button::LeftStick, BTN_THUMBL),
    (Button::RightStick, BTN_THUMBR),
];

/// `xpad` reports X as `BTN_X` (= `BTN_NORTH`) and Y as `BTN_Y`
/// (= `BTN_WEST`), the reverse of their positions.
const XPAD: [(Button, u16); 11] = {
    let mut buttons = POSITIONAL;
    buttons[2] = (Button::West, BTN_NORTH);
    buttons[3] = (Button::North, BTN_WEST);
    buttons
};

/// `hid-playstation` sets this bit on the version to mark its mapping, and
/// USB pads carry 0x0111 under it.
const PLAYSTATION_VERSION: u16 = 0x8111;

static XBOX: Layout = Layout {
    // A wired Xbox 360 pad: the id every gamepad stack knows, and the one
    // Android ships a dedicated key layout for.
    identity: Identity {
        name: "Microsoft X-Box 360 pad",
        vendor: 0x045e,
        product: 0x028e,
        version: 0x0114,
    },
    buttons: XPAD,
    stick: XPAD_STICK,
    trigger: BYTE,
    trigger_keys: false,
};

static PS4: Layout = Layout {
    // The second-revision DualShock 4 over USB.
    identity: Identity {
        name: "Sony Interactive Entertainment Wireless Controller",
        vendor: 0x054c,
        product: 0x09cc,
        version: PLAYSTATION_VERSION,
    },
    buttons: POSITIONAL,
    stick: BYTE,
    trigger: BYTE,
    trigger_keys: true,
};

static PS5: Layout = Layout {
    identity: Identity {
        name: "Sony Interactive Entertainment DualSense Wireless Controller",
        vendor: 0x054c,
        product: 0x0ce6,
        version: PLAYSTATION_VERSION,
    },
    buttons: POSITIONAL,
    stick: BYTE,
    trigger: BYTE,
    trigger_keys: true,
};

/// pid.codes' shared test id: no vendor's real product, so nothing maps it
/// to a branded pad.
const OPEN_VENDOR: u16 = 0x1209;
const OPEN_TEST_PRODUCT: u16 = 0x0001;

const GENERIC_IDENTITY: Identity = Identity {
    name: "rbx-native Virtual Gamepad",
    vendor: OPEN_VENDOR,
    product: OPEN_TEST_PRODUCT,
    version: 0x0001,
};

static GENERIC: Layout = Layout {
    identity: GENERIC_IDENTITY,
    buttons: POSITIONAL,
    stick: XPAD_STICK,
    trigger: BYTE,
    trigger_keys: true,
};

static HANDHELD: Layout = Layout {
    identity: Identity {
        name: "rbx-native Virtual Handheld",
        ..GENERIC_IDENTITY
    },
    buttons: POSITIONAL,
    stick: XPAD_STICK,
    trigger: BYTE,
    trigger_keys: true,
};

pub(super) fn layout(device: Device) -> Option<&'static Layout> {
    match device {
        Device::Generic => Some(&GENERIC),
        Device::Ps4 => Some(&PS4),
        Device::Ps5 => Some(&PS5),
        Device::Xbox => Some(&XBOX),
        Device::Handheld => Some(&HANDHELD),
        Device::TvRemote | Device::Quest => None,
    }
}

impl Layout {
    pub(super) fn key_code(&self, button: Button) -> Option<u16> {
        self.buttons
            .iter()
            .find(|(b, _)| *b == button)
            .map(|(_, code)| *code)
    }

    /// Every key code the device declares.
    pub(super) fn keys(&self) -> Vec<u16> {
        let mut keys: Vec<u16> = self.buttons.iter().map(|(_, code)| *code).collect();
        if self.trigger_keys {
            keys.extend([BTN_TL2, BTN_TR2]);
        }
        keys
    }

    /// Every absolute axis the device declares, with its range.
    pub(super) fn axes(&self) -> [(u16, Range); 8] {
        [
            (ABS_X, self.stick),
            (ABS_Y, self.stick),
            (ABS_RX, self.stick),
            (ABS_RY, self.stick),
            (ABS_Z, self.trigger),
            (ABS_RZ, self.trigger),
            (ABS_HAT0X, HAT),
            (ABS_HAT0Y, HAT),
        ]
    }

    /// The events that move the device from `prev` to `next`, ending in one
    /// `SYN_REPORT` so a reader sees the change as a single frame; empty
    /// when nothing the device reports changed.
    pub(super) fn encode(&self, prev: &PadState, next: &PadState) -> Vec<RawEvent> {
        let mut events = Vec::new();
        let mut put = |kind, code, old: i32, new: i32| {
            if old != new {
                events.push(RawEvent {
                    kind,
                    code,
                    value: new,
                });
            }
        };
        for (button, code) in self.buttons {
            put(
                EV_KEY,
                code,
                prev.pressed(button).into(),
                next.pressed(button).into(),
            );
        }
        for (side, (x, y)) in [
            (Side::Left, (ABS_X, ABS_Y)),
            (Side::Right, (ABS_RX, ABS_RY)),
        ] {
            let (old, new) = (prev.stick(side), next.stick(side));
            put(
                EV_ABS,
                x,
                scale_stick(self.stick, old.0),
                scale_stick(self.stick, new.0),
            );
            // evdev's Y grows downward.
            put(
                EV_ABS,
                y,
                scale_stick(self.stick, -old.1),
                scale_stick(self.stick, -new.1),
            );
        }
        for (side, axis, key) in [(Side::Left, ABS_Z, BTN_TL2), (Side::Right, ABS_RZ, BTN_TR2)] {
            let (old, new) = (
                scale_trigger(self.trigger, prev.trigger(side)),
                scale_trigger(self.trigger, next.trigger(side)),
            );
            put(EV_ABS, axis, old, new);
            if self.trigger_keys {
                put(
                    EV_KEY,
                    key,
                    (old > self.trigger.min).into(),
                    (new > self.trigger.min).into(),
                );
            }
        }
        let hat = |state: &PadState, minus, plus| {
            i32::from(state.pressed(plus)) - i32::from(state.pressed(minus))
        };
        put(
            EV_ABS,
            ABS_HAT0X,
            hat(prev, Button::DpadLeft, Button::DpadRight),
            hat(next, Button::DpadLeft, Button::DpadRight),
        );
        put(
            EV_ABS,
            ABS_HAT0Y,
            hat(prev, Button::DpadUp, Button::DpadDown),
            hat(next, Button::DpadUp, Button::DpadDown),
        );
        if !events.is_empty() {
            events.push(RawEvent {
                kind: EV_SYN,
                code: SYN_REPORT,
                value: 0,
            });
        }
        events
    }
}

/// `-1..=1` onto the range, centred where the driver rests: 0 for
/// `xpad`'s signed range, 128 for the PlayStation pads' byte.
fn scale_stick(range: Range, value: f32) -> i32 {
    let centre = (range.min + range.max + 1).div_euclid(2);
    let half = (range.max - centre) as f32;
    centre + (value.clamp(-1., 1.) * half).round() as i32
}

fn scale_trigger(range: Range, value: f32) -> i32 {
    range.min + (value.clamp(0., 1.) * (range.max - range.min) as f32).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: u16, value: i32) -> RawEvent {
        RawEvent {
            kind: EV_KEY,
            code,
            value,
        }
    }

    fn abs(code: u16, value: i32) -> RawEvent {
        RawEvent {
            kind: EV_ABS,
            code,
            value,
        }
    }

    const SYN: RawEvent = RawEvent {
        kind: EV_SYN,
        code: SYN_REPORT,
        value: 0,
    };

    #[test]
    fn no_change_sends_nothing() {
        let state = PadState::default();
        assert!(XBOX.encode(&state, &state).is_empty());
    }

    #[test]
    fn a_press_is_one_key_event_and_a_report() {
        let mut next = PadState::default();
        next.set(Button::South, true);
        assert_eq!(
            XBOX.encode(&PadState::default(), &next),
            vec![key(BTN_SOUTH, 1), SYN]
        );
        assert_eq!(
            XBOX.encode(&next, &PadState::default()),
            vec![key(BTN_SOUTH, 0), SYN]
        );
    }

    #[test]
    fn xbox_west_and_north_follow_xpad_not_their_positions() {
        assert_eq!(Device::Xbox.button_code(Button::West), Some(0x133));
        assert_eq!(Device::Xbox.button_code(Button::North), Some(0x134));
        assert_eq!(Device::Ps4.button_code(Button::West), Some(0x134));
        assert_eq!(Device::Ps5.button_code(Button::North), Some(0x133));
    }

    #[test]
    fn a_stick_move_is_batched_into_one_report_with_y_flipped() {
        let mut next = PadState::default();
        next.set_stick(Side::Left, 1., 1.);
        assert_eq!(
            XBOX.encode(&PadState::default(), &next),
            vec![abs(ABS_X, 32767), abs(ABS_Y, -32767), SYN]
        );
        let mut right = PadState::default();
        right.set_stick(Side::Right, -0.5, -1.);
        assert_eq!(
            PS4.encode(&PadState::default(), &right),
            // 128 is where hid-playstation's sticks rest.
            vec![abs(ABS_RX, 128 - 64), abs(ABS_RY, 255), SYN]
        );
    }

    #[test]
    fn a_resting_playstation_stick_reports_its_centre() {
        assert_eq!(scale_stick(BYTE, 0.), 128);
        assert_eq!(scale_stick(XPAD_STICK, 0.), 0);
    }

    #[test]
    fn playstation_triggers_also_press_their_keys() {
        let mut next = PadState::default();
        next.set_trigger(Side::Right, 0.5);
        assert_eq!(
            PS5.encode(&PadState::default(), &next),
            vec![abs(ABS_RZ, 128), key(BTN_TR2, 1), SYN]
        );
        assert_eq!(
            XBOX.encode(&PadState::default(), &next),
            vec![abs(ABS_RZ, 128), SYN]
        );
    }

    #[test]
    fn the_dpad_is_a_hat_and_opposite_directions_cancel() {
        let mut next = PadState::default();
        next.set(Button::DpadUp, true);
        next.set(Button::DpadRight, true);
        assert_eq!(
            GENERIC.encode(&PadState::default(), &next),
            vec![abs(ABS_HAT0X, 1), abs(ABS_HAT0Y, -1), SYN]
        );
        let mut both = next;
        both.set(Button::DpadLeft, true);
        assert_eq!(GENERIC.encode(&next, &both), vec![abs(ABS_HAT0X, 0), SYN]);
    }

    #[test]
    fn values_out_of_range_are_clamped() {
        let mut state = PadState::default();
        state.set_stick(Side::Left, 3., -3.);
        state.set_trigger(Side::Left, 2.);
        assert_eq!(state.stick(Side::Left), (1., -1.));
        assert_eq!(state.trigger(Side::Left), 1.);
    }

    #[test]
    fn every_emulated_device_has_a_layout_and_the_others_a_reason() {
        for device in Device::ALL {
            assert_eq!(layout(device).is_some(), device.unavailable().is_none());
            assert_eq!(Device::from_key(device.key()), Some(device));
        }
    }
}
