use super::{find, names, Physical, KEY_CODES};
use std::collections::HashSet;

#[test]
fn table_has_every_name() {
    assert_eq!(KEY_CODES.len(), 283);
}

#[test]
fn names_are_unique() {
    let unique: HashSet<&str> = names().collect();
    assert_eq!(unique.len(), KEY_CODES.len());
}

#[test]
fn letter_maps_to_ascii_and_vk() {
    let w = find("W").unwrap();
    assert_eq!(w.keysym, 0x77);
    assert_eq!(w.vk, 0x57);
    assert!(!w.extended);
}

#[test]
fn arrow_is_extended_and_typeable() {
    let left = find("Left").unwrap();
    assert!(left.extended);
    assert!(left.typeable());
}

#[test]
fn gamepad_button_is_not_typeable() {
    assert!(!find("ButtonA").unwrap().typeable());
}

#[test]
fn unknown_name_is_none() {
    assert!(find("Banana").is_none());
}

#[test]
fn first_name_is_none_key() {
    assert_eq!(names().next(), Some("None"));
}

fn place(name: &str) -> Physical {
    find(name).unwrap().physical().unwrap()
}

#[test]
fn keys_are_sent_by_their_place_on_a_us_keyboard() {
    // KEY_W, KEY_Z, KEY_A: AZERTY's Z, W and Q keys.
    assert_eq!(
        place("W"),
        Physical {
            evdev: 17,
            shift: false
        }
    );
    assert_eq!(place("W").x11(), 25);
    assert_eq!(place("Z").evdev, 44);
    assert_eq!(place("A").evdev, 30);
    assert_eq!(place("Zero").evdev, 11);
    assert_eq!(place("Nine").evdev, 10);
    assert_eq!(place("KeypadSeven").evdev, 71);
    assert_eq!(place("F1").evdev, 59);
    assert_eq!(place("F12").evdev, 88);
    assert_eq!(place("F15").evdev, 185);
    assert_eq!(
        place("Hash"),
        Physical {
            evdev: 4,
            shift: true
        }
    );
}

#[test]
fn scan_codes_follow_set_1() {
    assert_eq!(place("W").scancode(), Some((0x11, false)));
    assert_eq!(place("Up").scancode(), Some((0x48, true)));
    assert_eq!(place("RightControl").scancode(), Some((0x1d, true)));
    assert_eq!(place("F13").scancode(), Some((0x64, false)));
    assert_eq!(place("Help").scancode(), None);
}

#[test]
fn every_key_a_keyboard_has_has_a_place() {
    let placeless: Vec<&str> = KEY_CODES
        .iter()
        .filter(|key| key.keysym != 0 && key.physical().is_none())
        .map(|key| key.name)
        .filter(|name| !name.starts_with("World"))
        .collect();
    assert_eq!(placeless, ["Clear", "Mode", "Euro", "Undo"]);
}
