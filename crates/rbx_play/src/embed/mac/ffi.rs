//! Quartz window services, the Accessibility API and the
//! CoreFoundation glue between them, declared by hand the way the editor's
//! pointer lock declares its CoreGraphics calls.

use std::ffi::c_void;
use std::ptr::null;

use super::super::pin::{self, Frame};

pub(super) type CFTypeRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct CGPoint {
    pub(super) x: f64,
    pub(super) y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

/// A CoreFoundation constant only ever used by address.
#[repr(C)]
struct Opaque {
    _private: [u8; 0],
}

const UTF8: u32 = 0x0800_0100;
const SINT64: isize = 4;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
    fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const Opaque,
        value_callbacks: *const Opaque,
    ) -> CFTypeRef;
    fn CFNumberGetValue(number: CFTypeRef, kind: isize, out: *mut c_void) -> u8;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
    fn CFStringCreateWithBytes(
        allocator: CFTypeRef,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: u8,
    ) -> CFTypeRef;
    fn CFStringGetCString(string: CFTypeRef, buf: *mut u8, size: isize, encoding: u32) -> u8;
    fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
    fn CFDataGetLength(data: CFTypeRef) -> isize;
    static kCFBooleanTrue: CFTypeRef;
    static kCFBooleanFalse: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: Opaque;
    static kCFTypeDictionaryValueCallBacks: Opaque;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
    fn CGRectMakeWithDictionaryRepresentation(dict: CFTypeRef, rect: *mut CGRect) -> bool;
    fn CGGetDisplaysWithPoint(point: CGPoint, max: u32, displays: *mut u32, count: *mut u32)
        -> i32;
    fn CGDisplayCopyDisplayMode(display: u32) -> CFTypeRef;
    fn CGDisplayModeGetWidth(mode: CFTypeRef) -> usize;
    fn CGDisplayModeGetPixelWidth(mode: CFTypeRef) -> usize;
    fn CGDisplayModeRelease(mode: CFTypeRef);
    fn CGWindowListCreateImage(bounds: CGRect, option: u32, window: u32, image: u32) -> CFTypeRef;
    fn CGImageGetWidth(image: CFTypeRef) -> usize;
    fn CGImageGetHeight(image: CFTypeRef) -> usize;
    fn CGImageGetBytesPerRow(image: CFTypeRef) -> usize;
    fn CGImageGetBitsPerPixel(image: CFTypeRef) -> usize;
    fn CGImageGetDataProvider(image: CFTypeRef) -> CFTypeRef;
    fn CGDataProviderCopyData(provider: CFTypeRef) -> CFTypeRef;
    static CGRectNull: CGRect;
    static kCGWindowNumber: CFTypeRef;
    static kCGWindowOwnerPID: CFTypeRef;
    static kCGWindowOwnerName: CFTypeRef;
    static kCGWindowLayer: CFTypeRef;
    static kCGWindowBounds: CFTypeRef;
    static kCGWindowIsOnscreen: CFTypeRef;
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
    fn AXUIElementPerformAction(element: CFTypeRef, action: CFTypeRef) -> i32;
    fn AXValueCreate(kind: u32, value: *const c_void) -> CFTypeRef;
    /// Private but long-standing (window managers rely on it): the window
    /// server id of an Accessibility window element.
    fn _AXUIElementGetWindow(element: CFTypeRef, window: *mut u32) -> i32;
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
}

/// An owned CoreFoundation reference, released on drop.
pub(super) struct Cf(pub(super) CFTypeRef);

// SAFETY: CoreFoundation reference counting is thread-safe, and the window
// and Accessibility objects held are only used through calls that may be
// made from any thread.
unsafe impl Send for Cf {}

impl Cf {
    pub(super) fn new(reference: CFTypeRef) -> Option<Self> {
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

/// Asks whether this process may use the Accessibility API, showing
/// macOS's "allow in System Settings" prompt when it may not.
pub(super) fn trusted_or_prompt() -> bool {
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

/// `kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements`,
/// front to back.
pub(super) const ON_SCREEN: u32 = 1 | 1 << 4;
/// `kCGWindowListOptionIncludingWindow`.
const INCLUDING: u32 = 1 << 3;

/// One entry of the window server's list.
pub(super) struct WindowInfo {
    pub(super) id: u32,
    pub(super) pid: i32,
    pub(super) owner: String,
    pub(super) layer: i64,
    pub(super) frame: Frame,
    pub(super) onscreen: bool,
}

/// The window server's windows for `option` relative to `window`.
pub(super) fn windows(option: u32, window: u32) -> Vec<WindowInfo> {
    // SAFETY: the array is owned and released by `Cf`; its entries are
    // dictionaries borrowed from it while it lives.
    unsafe {
        let Some(list) = Cf::new(CGWindowListCopyWindowInfo(option, window)) else {
            return Vec::new();
        };
        (0..CFArrayGetCount(list.0))
            .filter_map(|i| info(CFArrayGetValueAtIndex(list.0, i)))
            .collect()
    }
}

/// `window`'s entry, on screen or not.
pub(super) fn window(window: u32) -> Option<WindowInfo> {
    windows(INCLUDING, window)
        .into_iter()
        .find(|info| info.id == window)
}

/// # Safety
/// `dict` is a live window-info dictionary.
unsafe fn info(dict: CFTypeRef) -> Option<WindowInfo> {
    // SAFETY: per the caller; every value is type-checked by the getter.
    unsafe {
        let number = |key: CFTypeRef| {
            let value = CFDictionaryGetValue(dict, key);
            let mut out = 0i64;
            (!value.is_null() && CFNumberGetValue(value, SINT64, (&raw mut out).cast()) != 0)
                .then_some(out)
        };
        let owner = CFDictionaryGetValue(dict, kCGWindowOwnerName);
        let mut buf = [0u8; 256];
        let owner = match !owner.is_null()
            && CFStringGetCString(owner, buf.as_mut_ptr(), buf.len() as isize, UTF8) != 0
        {
            true => std::ffi::CStr::from_bytes_until_nul(&buf)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            false => String::new(),
        };
        let bounds = CFDictionaryGetValue(dict, kCGWindowBounds);
        let mut rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: 0.0,
                height: 0.0,
            },
        };
        if bounds.is_null() || !CGRectMakeWithDictionaryRepresentation(bounds, &mut rect) {
            return None;
        }
        let onscreen = CFDictionaryGetValue(dict, kCGWindowIsOnscreen);
        Some(WindowInfo {
            id: u32::try_from(number(kCGWindowNumber)?).ok()?,
            pid: i32::try_from(number(kCGWindowOwnerPID)?).ok()?,
            owner,
            layer: number(kCGWindowLayer).unwrap_or(0),
            frame: Frame {
                x: rect.origin.x,
                y: rect.origin.y,
                width: rect.size.width,
                height: rect.size.height,
            },
            onscreen: !onscreen.is_null() && CFBooleanGetValue(onscreen) != 0,
        })
    }
}

/// The backing scale of the display showing `frame`'s centre.
pub(super) fn display_scale(frame: Frame) -> f64 {
    let centre = CGPoint {
        x: frame.x + frame.width / 2.0,
        y: frame.y + frame.height / 2.0,
    };
    let (mut display, mut count) = (0u32, 0u32);
    // SAFETY: out pointers to locals; the mode is released after use.
    unsafe {
        if CGGetDisplaysWithPoint(centre, 1, &mut display, &mut count) != 0 || count == 0 {
            return 1.0;
        }
        let mode = CGDisplayCopyDisplayMode(display);
        if mode.is_null() {
            return 1.0;
        }
        let scale = pin::mode_scale(
            CGDisplayModeGetPixelWidth(mode),
            CGDisplayModeGetWidth(mode),
        );
        CGDisplayModeRelease(mode);
        scale
    }
}

/// `window`'s pixels without its shadow, at the display's resolution:
/// `(width, height, bytes per row, 32-bit BGRA bytes)`.
pub(super) fn window_image(window: u32) -> Option<(usize, usize, usize, Vec<u8>)> {
    /// `kCGWindowImageBoundsIgnoreFraming | kCGWindowImageBestResolution`.
    const IMAGE: u32 = 1 | 1 << 3;
    // SAFETY: the image and its data are owned by `Cf`s; the provider is
    // borrowed from the image while it lives.
    unsafe {
        let image = Cf::new(CGWindowListCreateImage(
            CGRectNull, INCLUDING, window, IMAGE,
        ))?;
        if CGImageGetBitsPerPixel(image.0) != 32 {
            return None;
        }
        let data = Cf::new(CGDataProviderCopyData(CGImageGetDataProvider(image.0)))?;
        let len = usize::try_from(CFDataGetLength(data.0)).ok()?;
        let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data.0), len).to_vec();
        Some((
            CGImageGetWidth(image.0),
            CGImageGetHeight(image.0),
            CGImageGetBytesPerRow(image.0),
            bytes,
        ))
    }
}

/// An Accessibility element: an application or one of its windows.
pub(super) struct Ax(Cf);

impl Ax {
    pub(super) fn app(pid: i32) -> Option<Self> {
        // SAFETY: plain call; a null result is handled.
        Cf::new(unsafe { AXUIElementCreateApplication(pid) }).map(Ax)
    }

    /// The application's window the window server numbers `id`, else its
    /// first one.
    pub(super) fn window(&self, id: u32) -> Option<Ax> {
        let attribute = string("AXWindows");
        let mut value: CFTypeRef = null();
        // SAFETY: `value` is an out pointer; the array it gets is owned by
        // `list`, and each element taken is retained before the list goes.
        unsafe {
            if AXUIElementCopyAttributeValue(self.0 .0, attribute.0, &mut value) != 0 {
                return None;
            }
            let list = Cf::new(value)?;
            let windows: Vec<CFTypeRef> = (0..CFArrayGetCount(list.0))
                .map(|i| CFArrayGetValueAtIndex(list.0, i))
                .collect();
            let numbered = |element: CFTypeRef| {
                let mut number = 0u32;
                _AXUIElementGetWindow(element, &mut number) == 0 && number == id
            };
            let found = windows
                .iter()
                .copied()
                .find(|&element| numbered(element))
                .or_else(|| windows.first().copied())?;
            Cf::new(CFRetain(found)).map(Ax)
        }
    }

    fn set(&self, attribute: &str, value: CFTypeRef) -> bool {
        let attribute = string(attribute);
        // SAFETY: both references are live for the call.
        unsafe { AXUIElementSetAttributeValue(self.0 .0, attribute.0, value) == 0 }
    }

    pub(super) fn set_bool(&self, attribute: &str, on: bool) -> bool {
        // SAFETY: reads of CF constants.
        let value = unsafe {
            match on {
                true => kCFBooleanTrue,
                false => kCFBooleanFalse,
            }
        };
        self.set(attribute, value)
    }

    /// Moves and sizes a window element to `frame`.
    pub(super) fn set_frame(&self, frame: Frame) -> bool {
        let at = CGPoint {
            x: frame.x,
            y: frame.y,
        };
        let size = CGSize {
            width: frame.width,
            height: frame.height,
        };
        // SAFETY: `kAXValueCGPointType` (1) and `kAXValueCGSizeType` (2)
        // read a CGPoint and a CGSize from the pointers given.
        let (at, size) = unsafe {
            (
                Cf::new(AXValueCreate(1, (&raw const at).cast())),
                Cf::new(AXValueCreate(2, (&raw const size).cast())),
            )
        };
        let (Some(at), Some(size)) = (at, size) else {
            return false;
        };
        self.set("AXPosition", at.0) & self.set("AXSize", size.0)
    }

    /// Brings a window element to the front without activating its app.
    pub(super) fn raise(&self) -> bool {
        let action = string("AXRaise");
        // SAFETY: both references are live for the call.
        unsafe { AXUIElementPerformAction(self.0 .0, action.0) == 0 }
    }
}
