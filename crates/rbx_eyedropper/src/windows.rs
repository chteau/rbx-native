use std::cell::RefCell;
use std::ptr::null;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN,
};

const CLR_INVALID: u32 = 0xFFFF_FFFF;

struct State {
    thread: u32,
    outcome: Option<crate::Outcome>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Records the first outcome; later ones are ignored so `done` always sees
/// exactly one result.
fn record(outcome: crate::Outcome) {
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.outcome.get_or_insert(outcome);
        }
    });
}

/// Stops the message loop.
fn quit() {
    STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            // SAFETY: posting a quit to our own thread's queue.
            unsafe { PostThreadMessageW(state.thread, WM_QUIT, 0, 0) };
        }
    });
}

/// Records `outcome` and stops at once: for cancels, whose button release
/// (if any) does no harm reaching the window below.
fn finish(outcome: crate::Outcome) {
    record(outcome);
    quit();
}

fn resolved() -> bool {
    STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.outcome.is_some()))
}

fn read_pixel(x: i32, y: i32) -> crate::Outcome {
    // SAFETY: a null window yields the whole-screen DC, released below.
    let color = unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let color = GetPixel(dc, x, y);
        ReleaseDC(std::ptr::null_mut(), dc);
        color
    };
    if color == CLR_INVALID {
        Err("The colour under the pointer could not be read.".into())
    } else {
        Ok(Some(crate::from_colorref(color)))
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        match wparam as u32 {
            WM_LBUTTONDOWN => {
                if !resolved() {
                    // SAFETY: for WH_MOUSE_LL, lparam points to an MSLLHOOKSTRUCT.
                    let pt = unsafe { (*(lparam as *const MSLLHOOKSTRUCT)).pt };
                    record(read_pixel(pt.x, pt.y));
                }
                return 1;
            }
            // The loop ends on the release, so the click's second half is
            // swallowed too rather than reaching the window under the pointer.
            WM_LBUTTONUP => {
                if resolved() {
                    quit();
                    return 1;
                }
            }
            WM_RBUTTONDOWN => {
                finish(Ok(None));
                return 1;
            }
            _ => {}
        }
    }
    // SAFETY: forwarding the unmodified hook arguments.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        // SAFETY: for WH_KEYBOARD_LL, lparam points to a KBDLLHOOKSTRUCT.
        let vk = unsafe { (*(lparam as *const KBDLLHOOKSTRUCT)).vkCode };
        if vk == u32::from(VK_ESCAPE) {
            finish(Ok(None));
            return 1;
        }
    }
    // SAFETY: forwarding the unmodified hook arguments.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

pub(crate) fn pick(done: Box<dyn FnOnce(crate::Outcome) + Send>) {
    std::thread::spawn(move || {
        // SAFETY: plain Win32 calls on this thread; hooks are removed before returning.
        let outcome = unsafe {
            STATE.with(|s| {
                *s.borrow_mut() = Some(State {
                    thread: GetCurrentThreadId(),
                    outcome: None,
                });
            });
            let module = GetModuleHandleW(null());
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
            let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
            let installed = !mouse.is_null() && !keyboard.is_null();
            if installed {
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
            }
            if !mouse.is_null() {
                UnhookWindowsHookEx(mouse);
            }
            if !keyboard.is_null() {
                UnhookWindowsHookEx(keyboard);
            }
            let result = STATE.with(|s| s.borrow_mut().take().and_then(|st| st.outcome));
            match (installed, result) {
                (_, Some(r)) => r,
                _ => Err("The screen colour picker could not be started.".into()),
            }
        };
        done(outcome);
    });
}
