//! The Roblox player on macOS, which Play can't put in the viewport: macOS
//! has no way to place another app's window inside ours (`docs` explain
//! the hunt). Play instead brings the player up full screen and leaves it
//! to the user; the editor's viewport says so, and the in-game dev panel
//! (`probe/panel.luau`) carries the editor's Output and debugger along.
//!
//! Full screen goes through the Accessibility API (`AXFullScreen` on the
//! player's game window), the only way to ask another app for it; without
//! the permission the player simply stays windowed. Bringing it to the
//! front needs no permission.

use std::ffi::c_void;
use std::ptr::null;
use std::time::{Duration, Instant};

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use objc2_foundation::NSString;

/// The Roblox player's bundle identifier.
const BUNDLE: &str = "com.roblox.RobloxPlayer";

/// The smallest window taken for the game's rather than the launcher's
/// splash or a dialog, in points.
const MIN_GAME: (f64, f64) = (500.0, 350.0);

type CFTypeRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGSize {
    width: f64,
    height: f64,
}

/// A CoreFoundation constant only ever used by address.
#[repr(C)]
struct Opaque {
    _private: [u8; 0],
}

const UTF8: u32 = 0x0800_0100;
/// `kAXValueCGSizeType`.
const AX_SIZE: u32 = 2;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const Opaque,
        value_callbacks: *const Opaque,
    ) -> CFTypeRef;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
    fn CFStringCreateWithBytes(
        allocator: CFTypeRef,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: u8,
    ) -> CFTypeRef;
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: Opaque;
    static kCFTypeDictionaryValueCallBacks: Opaque;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: CFTypeRef,
        attribute: CFTypeRef,
        value: CFTypeRef,
    ) -> i32;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, out: *mut c_void) -> u8;
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
}

/// An owned CoreFoundation reference, released on drop.
struct Cf(CFTypeRef);

impl Cf {
    fn new(reference: CFTypeRef) -> Option<Self> {
        (!reference.is_null()).then_some(Cf(reference))
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a non-null reference this value owns.
        unsafe { CFRelease(self.0) };
    }
}

fn string(text: &str) -> Cf {
    // SAFETY: `text` is valid for its length; the result is checked below.
    let made =
        unsafe { CFStringCreateWithBytes(null(), text.as_ptr(), text.len() as isize, UTF8, 0) };
    Cf::new(made).expect("CFStringCreateWithBytes on valid UTF-8")
}

/// Whether this process may use the Accessibility API; `prompt` shows
/// macOS's "allow in System Settings" dialog when it may not.
fn trusted(prompt: bool) -> bool {
    if !prompt {
        // SAFETY: a null options dictionary is allowed.
        return unsafe { AXIsProcessTrustedWithOptions(null()) != 0 };
    }
    // SAFETY: a one-entry dictionary of CF constants, released after use.
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let Some(options) = Cf::new(CFDictionaryCreate(
            null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &raw const kCFTypeDictionaryKeyCallBacks,
            &raw const kCFTypeDictionaryValueCallBacks,
        )) else {
            return false;
        };
        AXIsProcessTrustedWithOptions(options.0) != 0
    }
}

/// `element`'s `attribute`, owned.
fn copy(element: CFTypeRef, attribute: &str) -> Option<Cf> {
    let attribute = string(attribute);
    let mut value: CFTypeRef = null();
    // SAFETY: `value` is an out pointer whose result this function owns.
    let status = unsafe { AXUIElementCopyAttributeValue(element, attribute.0, &mut value) };
    if status != 0 {
        return None;
    }
    Cf::new(value)
}

fn size(window: CFTypeRef) -> CGSize {
    let mut out = CGSize::default();
    if let Some(value) = copy(window, "AXSize") {
        // SAFETY: an AXSize value holds a CGSize.
        unsafe { AXValueGetValue(value.0, AX_SIZE, (&raw mut out).cast()) };
    }
    out
}

fn is_full_screen(window: CFTypeRef) -> bool {
    // SAFETY: AXFullScreen is a CFBoolean.
    copy(window, "AXFullScreen").is_some_and(|value| unsafe { CFBooleanGetValue(value.0) != 0 })
}

/// The running Roblox player, if any.
fn player() -> Option<objc2::rc::Retained<NSRunningApplication>> {
    let bundle = NSString::from_str(BUNDLE);
    NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle)
        .iter()
        .find(|app| !app.isTerminated())
}

/// What `full_screen` managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullScreen {
    /// The game's window went full screen (or already was).
    Done,
    /// No Accessibility permission: the player stays windowed.
    NoAccess,
    /// No game window showed up in time.
    NotFound,
}

/// Waits up to `timeout` for the player's game window, then asks for full
/// screen. Asks for the Accessibility permission once (macOS's own prompt).
/// `give_up` is polled to stop early (Play stopped).
pub fn full_screen(timeout: Duration, give_up: &dyn Fn() -> bool) -> FullScreen {
    if !trusted(true) {
        return FullScreen::NoAccess;
    }
    let deadline = Instant::now() + timeout;
    let attribute = string("AXFullScreen");
    while Instant::now() < deadline && !give_up() {
        if let Some(app) = player() {
            if let Some(element) = Cf::new(
                // SAFETY: plain call; a null result is handled.
                unsafe { AXUIElementCreateApplication(app.processIdentifier()) },
            ) {
                if let Some(windows) = copy(element.0, "AXWindows") {
                    // SAFETY: AXWindows is a CFArray of window elements,
                    // borrowed from `windows` while it lives.
                    let count = unsafe { CFArrayGetCount(windows.0) };
                    for i in 0..count {
                        // SAFETY: `i` is within the array's count.
                        let window = unsafe { CFArrayGetValueAtIndex(windows.0, i) };
                        let CGSize { width, height } = size(window);
                        if width < MIN_GAME.0 || height < MIN_GAME.1 {
                            continue;
                        }
                        if is_full_screen(window) {
                            return FullScreen::Done;
                        }
                        // SAFETY: both references live for the call.
                        let set = unsafe {
                            AXUIElementSetAttributeValue(window, attribute.0, kCFBooleanTrue)
                        };
                        if set == 0 {
                            return FullScreen::Done;
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    FullScreen::NotFound
}

/// Brings the player to the front, if it runs. No permission needed.
pub fn bring_to_front() -> bool {
    player().is_some_and(|app| app.activateWithOptions(NSApplicationActivationOptions::empty()))
}

/// Quits the player (Stop): asks politely, as Cmd+Q would.
pub fn quit() {
    if let Some(app) = player() {
        app.terminate();
    }
}
