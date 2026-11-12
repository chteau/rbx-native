//! The display-free half of the macOS back end: where the client's window
//! goes on screen, which windows are the player's, when it has to be raised
//! over the editor, and which Mac key code a US key is. Built for tests on
//! every platform, so CI's Linux job checks it too.
//!
//! Quartz window services and the Accessibility API both measure in global
//! display points with the origin at the top-left of the main display, so
//! nothing here flips y: only AppKit's `NSWindow` frames are bottom-left,
//! and this back end never reads one.

use super::Rect;

/// Height of a standard macOS title bar, in points. The player keeps its
/// own, so the game draws below it; not measured on the player itself.
pub(crate) const TITLE_BAR: f64 = 28.0;

/// `kCGWindowOwnerName` of the Roblox player's windows (the app's name, not
/// its executable's). Exact matches only: "Roblox Studio" is not a client.
const CLIENT_OWNERS: &[&str] = &["Roblox", "RobloxPlayer", "Roblox Player"];

/// The smallest window taken for the game's: tool tips and the
/// updater's progress strip are smaller.
const MIN_CLIENT_SIDE: f64 = 100.0;

/// A window frame in global display points, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Frame {
    /// Equal within a point: the window server rounds frames.
    pub(crate) fn near(self, other: Frame) -> bool {
        [
            self.x - other.x,
            self.y - other.y,
            self.width - other.width,
            self.height - other.height,
        ]
        .iter()
        .all(|d| d.abs() <= 1.0)
    }
}

/// `rect`, in physical pixels of the editor window, as a global frame: the
/// editor's origin plus the rect in points. The editor's window has a
/// full-size content view (its title bar is drawn by the app), so its frame
/// origin is its content origin.
pub(crate) fn screen_frame(editor: Frame, rect: Rect, scale: f64) -> Frame {
    Frame {
        x: editor.x + f64::from(rect.x) / scale,
        y: editor.y + f64::from(rect.y) / scale,
        width: f64::from(rect.width) / scale,
        height: f64::from(rect.height) / scale,
    }
}

/// A display mode's backing scale: pixels per point, 1 when unknown.
pub(crate) fn mode_scale(pixel_width: usize, width: usize) -> f64 {
    if pixel_width == 0 || width == 0 {
        return 1.0;
    }
    pixel_width as f64 / width as f64
}

/// Whether a window is the player's game window: owned by the player app,
/// on the normal window layer, and big enough.
pub(crate) fn is_client(owner: &str, layer: i64, frame: Frame) -> bool {
    layer == 0
        && CLIENT_OWNERS.contains(&owner)
        && frame.width >= MIN_CLIENT_SIDE
        && frame.height >= MIN_CLIENT_SIDE
}

/// Whether the client must be raised: the editor's app is the active one
/// (its windows are in front, not necessarily the editor's own: Home or
/// Settings may be) and the client sits behind the editor. `front_to_back`
/// is the normal-layer windows as `(window, pid)`. Another app's window in
/// front means the user is elsewhere, and the client stays where it is.
pub(crate) fn needs_raise(front_to_back: &[(u32, i32)], me: i32, editor: u32, client: u32) -> bool {
    let at = |id: u32| front_to_back.iter().position(|&(w, _)| w == id);
    let front_is_mine = front_to_back.first().is_some_and(|&(_, pid)| pid == me);
    match (at(editor), at(client)) {
        (Some(editor), Some(client)) => front_is_mine && client > editor,
        _ => false,
    }
}

/// The game's area in pixels: the window below its title bar.
pub(crate) fn content_size(frame: Frame, scale: f64) -> (u32, u32) {
    let pixels = |points: f64| (points.max(0.0) * scale).round() as u32;
    (pixels(frame.width), pixels(frame.height - TITLE_BAR))
}

/// A point in the game's pixels as a global point.
pub(crate) fn client_point(frame: Frame, scale: f64, (x, y): (i32, i32)) -> (f64, f64) {
    (
        frame.x + f64::from(x) / scale,
        frame.y + TITLE_BAR + f64::from(y) / scale,
    )
}

/// The rows of a 32-bit BGRA/BGRX image below `top`, as tightly packed
/// RGBA; `bytes_per_row` may pad each row.
pub(crate) fn crop_rows(
    bytes: &[u8],
    bytes_per_row: usize,
    width: usize,
    height: usize,
    top: usize,
) -> Vec<u8> {
    (top.min(height)..height)
        .filter_map(|row| bytes.get(row * bytes_per_row..row * bytes_per_row + width * 4))
        .flat_map(super::bgrx_to_rgba)
        .collect()
}

/// `kVK_Shift`.
pub(crate) const MAC_SHIFT: u16 = 0x38;

/// The Mac virtual key code (`kVK_*` in `Events.h`) of the key at an evdev
/// code's place on a US keyboard; `None` for keys a Mac keyboard lacks.
pub(crate) fn mac_keycode(evdev: u16) -> Option<u16> {
    // evdev 2..=11 are the digits 1..9, 0.
    const DIGITS: [u16; 10] = [0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1a, 0x1c, 0x19, 0x1d];
    // evdev 16..=25: Q W E R T Y U I O P.
    const TOP_ROW: [u16; 10] = [0x0c, 0x0d, 0x0e, 0x0f, 0x11, 0x10, 0x20, 0x22, 0x1f, 0x23];
    // evdev 30..=38: A S D F G H J K L.
    const HOME_ROW: [u16; 9] = [0x00, 0x01, 0x02, 0x03, 0x05, 0x04, 0x26, 0x28, 0x25];
    // evdev 44..=50: Z X C V B N M.
    const BOTTOM_ROW: [u16; 7] = [0x06, 0x07, 0x08, 0x09, 0x0b, 0x2d, 0x2e];
    // evdev 59..=68: F1..F10.
    const F1_F10: [u16; 10] = [0x7a, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6d];
    // evdev 71..=83: keypad 7 8 9 - 4 5 6 + 1 2 3 0 .
    const KEYPAD: [u16; 13] = [
        0x59, 0x5b, 0x5c, 0x4e, 0x56, 0x57, 0x58, 0x45, 0x53, 0x54, 0x55, 0x52, 0x41,
    ];
    let at = |table: &[u16], first: u16| table.get(usize::from(evdev - first)).copied();
    match evdev {
        1 => Some(0x35),
        2..=11 => at(&DIGITS, 2),
        12 => Some(0x1b),
        13 => Some(0x18),
        14 => Some(0x33),
        15 => Some(0x30),
        16..=25 => at(&TOP_ROW, 16),
        26 => Some(0x21),
        27 => Some(0x1e),
        28 => Some(0x24),
        29 => Some(0x3b),
        30..=38 => at(&HOME_ROW, 30),
        39 => Some(0x29),
        40 => Some(0x27),
        41 => Some(0x32),
        42 => Some(MAC_SHIFT),
        43 => Some(0x2a),
        44..=50 => at(&BOTTOM_ROW, 44),
        51 => Some(0x2b),
        52 => Some(0x2f),
        53 => Some(0x2c),
        54 => Some(0x3c),
        55 => Some(0x43),
        56 => Some(0x3a),
        57 => Some(0x31),
        58 => Some(0x39),
        59..=68 => at(&F1_F10, 59),
        71..=83 => at(&KEYPAD, 71),
        87 => Some(0x67),
        88 => Some(0x6f),
        96 => Some(0x4c),
        97 => Some(0x3e),
        98 => Some(0x4b),
        100 => Some(0x3d),
        102 => Some(0x73),
        103 => Some(0x7e),
        104 => Some(0x74),
        105 => Some(0x7b),
        106 => Some(0x7c),
        107 => Some(0x77),
        108 => Some(0x7d),
        109 => Some(0x79),
        // Insert sits where a Mac keyboard has Help.
        110 => Some(0x72),
        111 => Some(0x75),
        117 => Some(0x51),
        125 => Some(0x37),
        126 => Some(0x36),
        183 => Some(0x69),
        184 => Some(0x6b),
        185 => Some(0x71),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDITOR: Frame = Frame {
        x: 100.0,
        y: 50.0,
        width: 1200.0,
        height: 800.0,
    };

    #[test]
    fn rect_lands_at_the_editor_origin_plus_points() {
        let rect = Rect {
            x: 400,
            y: 200,
            width: 1600,
            height: 900,
        };
        assert_eq!(
            screen_frame(EDITOR, rect, 2.0),
            Frame {
                x: 300.0,
                y: 150.0,
                width: 800.0,
                height: 450.0
            }
        );
        assert_eq!(screen_frame(EDITOR, rect, 1.0).x, 500.0);
    }

    #[test]
    fn y_grows_downward_on_a_display_above_the_main_one() {
        let above = Frame {
            y: -900.0,
            ..EDITOR
        };
        let rect = Rect {
            x: 0,
            y: 60,
            width: 10,
            height: 10,
        };
        assert_eq!(screen_frame(above, rect, 2.0).y, -870.0);
    }

    #[test]
    fn scale_is_pixels_per_point() {
        assert_eq!(mode_scale(2880, 1440), 2.0);
        assert_eq!(mode_scale(3360, 1680), 2.0);
        assert_eq!(mode_scale(1920, 1920), 1.0);
        assert_eq!(mode_scale(0, 0), 1.0);
    }

    #[test]
    fn near_tolerates_rounding_only() {
        let moved = Frame { x: 100.6, ..EDITOR };
        assert!(EDITOR.near(moved));
        assert!(!EDITOR.near(Frame { x: 102.0, ..EDITOR }));
    }

    #[test]
    fn only_the_players_big_normal_windows_match() {
        assert!(is_client("Roblox", 0, EDITOR));
        assert!(is_client("RobloxPlayer", 0, EDITOR));
        assert!(!is_client("Roblox Studio", 0, EDITOR));
        assert!(!is_client("RobloxStudio", 0, EDITOR));
        assert!(!is_client("Roblox", 3, EDITOR));
        let tiny = Frame {
            width: 80.0,
            ..EDITOR
        };
        assert!(!is_client("Roblox", 0, tiny));
    }

    #[test]
    fn raises_only_when_the_editor_app_is_in_front_of_a_shown_client() {
        const ME: i32 = 10;
        const ROBLOX: i32 = 20;
        const SAFARI: i32 = 30;
        // Editor (1) in front, client (2) behind it.
        assert!(needs_raise(&[(1, ME), (2, ROBLOX), (9, SAFARI)], ME, 1, 2));
        // Another of the editor's windows (Settings) in front of both.
        assert!(needs_raise(&[(5, ME), (1, ME), (2, ROBLOX)], ME, 1, 2));
        // Already over the editor.
        assert!(!needs_raise(&[(2, ROBLOX), (1, ME)], ME, 1, 2));
        assert!(!needs_raise(&[(5, ME), (2, ROBLOX), (1, ME)], ME, 1, 2));
        // Safari in front: the user is elsewhere.
        assert!(!needs_raise(&[(7, SAFARI), (1, ME), (2, ROBLOX)], ME, 1, 2));
        // The client isn't on screen, or nothing is.
        assert!(!needs_raise(&[(1, ME), (9, SAFARI)], ME, 1, 2));
        assert!(!needs_raise(&[], ME, 1, 2));
    }

    #[test]
    fn client_points_start_below_the_title_bar() {
        let frame = Frame {
            x: 10.0,
            y: 20.0,
            width: 400.0,
            height: 328.0,
        };
        assert_eq!(content_size(frame, 2.0), (800, 600));
        assert_eq!(client_point(frame, 2.0, (100, 50)), (60.0, 73.0));
    }

    #[test]
    fn crop_drops_rows_and_row_padding() {
        // 1x3 image, 8 bytes a row (4 of padding), BGRA.
        let bytes = [
            1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0, 9, 10, 11, 12, 0, 0, 0, 0,
        ];
        assert_eq!(
            crop_rows(&bytes, 8, 1, 3, 1),
            vec![7, 6, 5, 255, 11, 10, 9, 255]
        );
        assert!(crop_rows(&bytes, 8, 1, 3, 5).is_empty());
    }

    #[test]
    fn keys_map_to_their_mac_places() {
        assert_eq!(mac_keycode(30), Some(0x00)); // A
        assert_eq!(mac_keycode(17), Some(0x0d)); // W
        assert_eq!(mac_keycode(2), Some(0x12)); // 1
        assert_eq!(mac_keycode(11), Some(0x1d)); // 0
        assert_eq!(mac_keycode(57), Some(0x31)); // Space
        assert_eq!(mac_keycode(103), Some(0x7e)); // Up
        assert_eq!(mac_keycode(68), Some(0x6d)); // F10
        assert_eq!(mac_keycode(83), Some(0x41)); // keypad .
        assert_eq!(mac_keycode(crate::keycode::LEFT_SHIFT), Some(MAC_SHIFT));
        assert_eq!(mac_keycode(119), None); // Pause
    }
}
