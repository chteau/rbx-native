//! Quartz events posted where hardware events enter (`kCGHIDEventTap`).

use std::ptr::null;

pub(super) use super::ffi::CGPoint;
use super::ffi::{CFTypeRef, Cf};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: CFTypeRef) -> CFTypeRef;
    fn CGEventGetLocation(event: CFTypeRef) -> CGPoint;
    fn CGEventCreateMouseEvent(source: CFTypeRef, kind: u32, at: CGPoint, button: u32)
        -> CFTypeRef;
    fn CGEventCreateKeyboardEvent(source: CFTypeRef, key: u16, down: bool) -> CFTypeRef;
    fn CGEventCreateScrollWheelEvent2(
        source: CFTypeRef,
        units: u32,
        wheels: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> CFTypeRef;
    fn CGEventKeyboardSetUnicodeString(event: CFTypeRef, len: usize, chars: *const u16);
    fn CGEventSetFlags(event: CFTypeRef, flags: u64);
    fn CGEventPost(tap: u32, event: CFTypeRef);
}

/// Where the cursor is, in global points.
pub(super) fn cursor() -> CGPoint {
    // SAFETY: an owned event, released by `Cf`.
    unsafe {
        match Cf::new(CGEventCreate(null())) {
            Some(event) => CGEventGetLocation(event.0),
            None => CGPoint { x: 0.0, y: 0.0 },
        }
    }
}

/// Posts `event` where a hardware event enters (`kCGHIDEventTap`).
fn post(event: CFTypeRef) -> bool {
    let Some(event) = Cf::new(event) else {
        return false;
    };
    // SAFETY: a live event.
    unsafe { CGEventPost(0, event.0) };
    true
}

/// A mouse event of `kind` (`CGEventType`) at `at` for `button`.
pub(super) fn post_mouse(kind: u32, at: CGPoint, button: u32) -> bool {
    // SAFETY: plain call; `post` handles a null result.
    post(unsafe { CGEventCreateMouseEvent(null(), kind, at, button) })
}

/// One wheel step, in lines.
pub(super) fn post_scroll(lines: i32) -> bool {
    // SAFETY: as in `post_mouse`.
    post(unsafe { CGEventCreateScrollWheelEvent2(null(), 1, 1, lines, 0, 0) })
}

/// A key press or release by Mac key code, with Shift held when `shift`.
pub(super) fn post_key(key: u16, down: bool, shift: bool) -> bool {
    /// `kCGEventFlagMaskShift`.
    const SHIFT: u64 = 0x0002_0000;
    // SAFETY: the event is only touched while non-null, then posted.
    unsafe {
        let event = CGEventCreateKeyboardEvent(null(), key, down);
        if !event.is_null() && shift {
            CGEventSetFlags(event, SHIFT);
        }
        post(event)
    }
}

/// Types one UTF-16 sequence as a key press and release.
pub(super) fn post_text(units: &[u16]) -> bool {
    [true, false].into_iter().all(|down| {
        // SAFETY: `units` is valid for its length during the call.
        unsafe {
            let event = CGEventCreateKeyboardEvent(null(), 0, down);
            if !event.is_null() {
                CGEventKeyboardSetUnicodeString(event, units.len(), units.as_ptr());
            }
            post(event)
        }
    })
}
