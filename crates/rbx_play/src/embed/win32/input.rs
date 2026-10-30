//! Pointer and key events into the embedded client through `SendInput`:
//! the same queue a person's mouse and keyboard feed, so the client reads
//! them as it reads real ones.

use std::thread;

use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, VkKeyScanW, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClientRect, SetCursorPos, SetForegroundWindow, GA_ROOT, WHEEL_DELTA,
};

use super::{failed, focus_window, hwnd};
use crate::embed::{Button, EmbedError, WindowId, CLICK_HOLD};
use crate::keycode::{KeyCode, LEFT_SHIFT};

/// One run of input into a client window.
pub struct Input {
    window: WindowId,
}

impl Input {
    pub fn open(window: WindowId) -> Result<Self, EmbedError> {
        Ok(Input { window })
    }

    /// The window's client area in pixels.
    pub fn size(&self) -> Result<(u32, u32), EmbedError> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `rect` is a valid out pointer; any handle value is accepted.
        if unsafe { GetClientRect(hwnd(self.window), &mut rect) } == 0 {
            return Err(failed("GetClientRect"));
        }
        let extent = |v: i32| u32::try_from(v.max(0)).unwrap_or(0);
        Ok((
            extent(rect.right - rect.left),
            extent(rect.bottom - rect.top),
        ))
    }

    /// Moves the cursor to `(x, y)` in the window's client pixels.
    pub fn move_to(&mut self, x: i32, y: i32) -> Result<(), EmbedError> {
        let mut at = POINT { x, y };
        // SAFETY: `at` is a valid in/out pointer.
        if unsafe { ClientToScreen(hwnd(self.window), &mut at) } == 0 {
            return Err(failed("ClientToScreen"));
        }
        // SAFETY: plain call; the cursor moves as if by the mouse.
        if unsafe { SetCursorPos(at.x, at.y) } == 0 {
            return Err(failed("SetCursorPos"));
        }
        Ok(())
    }

    pub fn button(&mut self, button: Button, down: bool) -> Result<(), EmbedError> {
        let flags = match (button, down) {
            (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
            (Button::Left, false) => MOUSEEVENTF_LEFTUP,
            (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
            (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
        };
        send(&[mouse(flags, 0)])
    }

    /// One wheel notch where the cursor is.
    pub fn scroll(&mut self, up: bool) -> Result<(), EmbedError> {
        let notch = if up {
            WHEEL_DELTA
        } else {
            WHEEL_DELTA.wrapping_neg()
        };
        send(&[mouse(MOUSEEVENTF_WHEEL, notch)])?;
        thread::sleep(CLICK_HOLD);
        Ok(())
    }

    /// Presses or releases `key` by its place on a US keyboard
    /// ([`KeyCode::physical`]): a scan code, which the client reads whatever
    /// the layout, with Shift around a shifted symbol. A name with no scan
    /// code is the key the current layout types it with (`on_layout`).
    pub fn key(&mut self, key: &KeyCode, down: bool) -> Result<(), EmbedError> {
        let place = key
            .physical()
            .and_then(|place| Some((place.scancode()?, place.shift)));
        let (event, shifted) = match place {
            Some((scan, shifted)) => (scanned(scan, !down), shifted),
            None => {
                let (vk, shifted) = on_layout(key)?;
                (keyboard(vk, key.extended, !down), shifted)
            }
        };
        self.take_keyboard()?;
        // evdev numbers Left Shift by its scan code.
        let shift = |up| scanned((LEFT_SHIFT, false), up);
        let mut events = Vec::with_capacity(3);
        if shifted && down {
            events.push(shift(false));
        }
        events.push(event);
        if shifted && down {
            events.push(shift(true));
        }
        send(&events)
    }

    /// Types `text` as Unicode key events, for a text field a game script
    /// cannot reach (the chat bar).
    pub fn type_text(&mut self, text: &str) -> Result<(), EmbedError> {
        self.take_keyboard()?;
        let events: Vec<INPUT> = text
            .encode_utf16()
            .flat_map(|unit| [unicode(unit, false), unicode(unit, true)])
            .collect();
        send(&events)
    }

    /// Keyboard events go to the foreground window's focus: the editor is
    /// brought forward and the client given the focus inside it.
    fn take_keyboard(&self) -> Result<(), EmbedError> {
        // SAFETY: plain calls on a handle; failure only means the editor
        // could not take the foreground, which focus_window reports.
        unsafe {
            let top = GetAncestor(hwnd(self.window), GA_ROOT);
            if !top.is_null() {
                SetForegroundWindow(top);
            }
        }
        focus_window(self.window)
    }
}

/// The virtual key that types `key` on the current layout: for a printable
/// character, the key the layout puts it on (and whether Shift is needed),
/// else `key.vk`.
fn on_layout(key: &KeyCode) -> Result<(u16, bool), EmbedError> {
    let (vk, shifted) = match u16::try_from(key.keysym) {
        Ok(c @ 0x20..=0x7e) => {
            // SAFETY: plain call.
            let found = unsafe { VkKeyScanW(c) };
            match u16::try_from(found) {
                Ok(scan) if scan & 0xff != 0xff => (scan & 0xff, scan & 0x100 != 0),
                _ => (key.vk, false),
            }
        }
        _ => (key.vk, false),
    };
    if vk == 0 {
        return Err(EmbedError::X11(format!("no key types {}", key.name)));
    }
    Ok((vk, shifted))
}

/// A key by its set-1 scan code.
fn scanned((scan, extended): (u16, bool), up: bool) -> INPUT {
    let mut flags = KEYEVENTF_SCANCODE;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse(flags: u32, data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn keyboard(vk: u16, extended: bool, up: bool) -> INPUT {
    // SAFETY: plain call.
    let scan = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) };
    let mut flags = 0;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: u16::try_from(scan).unwrap_or(0),
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn unicode(unit: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(events: &[INPUT]) -> Result<(), EmbedError> {
    let count = u32::try_from(events.len()).unwrap_or(u32::MAX);
    let size = i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or(i32::MAX);
    // SAFETY: `events` is a live slice of `count` INPUTs of `size` bytes.
    let sent = unsafe { SendInput(count, events.as_ptr(), size) };
    if sent != count {
        return Err(failed("SendInput"));
    }
    Ok(())
}
