//! The Win32 back end: puts `RobloxPlayerBeta.exe`'s window inside the
//! editor's viewport, frameless.
//!
//! The client's window is found with `EnumWindows`, matching each visible
//! top-level window's owning process image name against `CLIENT_EXES`. Its
//! caption, frame and popup styles are stripped, `WS_CHILD` is added, and
//! `SetParent` moves it under the editor's `HWND`; rects are then in the
//! parent's client-area pixels. Popup holes use a window region. Windows
//! joins the input queues of a cross-process parent and child, so a click
//! on the client focuses it natively and `FocusFollow` needs no thread.
//! `Drop` hands the window back to the desktop with its original styles.

use super::{visible_parts, EmbedError, Rect, WindowId};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HWND, LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, HRGN, RGN_ERROR, RGN_OR,
};
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetParent,
    SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNA, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_CLIPSIBLINGS, WS_DLGFRAME, WS_EX_APPWINDOW, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME,
    WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

#[cfg(target_pointer_width = "64")]
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW};
#[cfg(target_pointer_width = "32")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW as GetWindowLongPtrW, SetWindowLongW as SetWindowLongPtrW,
};

/// Lowercased executable file names of Roblox client processes.
pub const CLIENT_EXES: &[&str] = &["robloxplayerbeta.exe"];

fn failed(call: &str) -> EmbedError {
    EmbedError::X11(format!("{call} failed"))
}

fn hwnd(id: WindowId) -> HWND {
    id as HWND
}

fn process_exe(pid: u32) -> Option<String> {
    // SAFETY: OpenProcess has no memory preconditions; a null result is handled.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) };
    if process.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` is valid for `len` u16s and `process` is an open handle,
    // closed exactly once right after.
    let ok = unsafe {
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        ok
    };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    let name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
    Some(name.to_ascii_lowercase())
}

unsafe extern "system" fn collect(window: HWND, lparam: LPARAM) -> i32 {
    // SAFETY: `lparam` is the `&mut Vec<WindowId>` passed by `scan`, alive
    // for the whole EnumWindows call.
    let found = unsafe { &mut *(lparam as *mut Vec<WindowId>) };
    // SAFETY: `window` comes from EnumWindows; `pid` is a valid out pointer.
    let (visible, pid) = unsafe {
        let mut pid = 0u32;
        let visible = IsWindowVisible(window) != 0;
        GetWindowThreadProcessId(window, &mut pid);
        (visible, pid)
    };
    if visible
        && pid != 0
        && process_exe(pid).is_some_and(|exe| CLIENT_EXES.contains(&exe.as_str()))
    {
        found.push(window as WindowId);
    }
    TRUE
}

fn scan() -> Result<Vec<WindowId>, EmbedError> {
    let mut found: Vec<WindowId> = Vec::new();
    // SAFETY: the callback only dereferences `lparam` as this Vec, which
    // outlives the synchronous EnumWindows call.
    let ok = unsafe { EnumWindows(Some(collect), &mut found as *mut Vec<WindowId> as LPARAM) };
    if ok == 0 {
        return Err(failed("EnumWindows"));
    }
    Ok(found)
}

/// A client window living inside the editor.
pub struct Embedded {
    window: WindowId,
    style: isize,
    ex_style: isize,
}

impl Embedded {
    /// Polls every 100 ms until a visible client top-level window not in
    /// `ignore` exists, or `timeout` passes; then strips its frame, makes it
    /// a child of `parent` and places it at `rect`.
    pub fn attach(
        parent: WindowId,
        rect: Rect,
        ignore: &[WindowId],
        timeout: Duration,
    ) -> Result<Self, EmbedError> {
        let deadline = Instant::now() + timeout;
        let window = loop {
            if let Some(w) = scan()?.into_iter().find(|w| !ignore.contains(w)) {
                break w;
            }
            if Instant::now() >= deadline {
                return Err(EmbedError::NotFound);
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let h = hwnd(window);
        // SAFETY: plain Win32 calls on window handles; invalid handles make
        // them fail, which is checked.
        let (style, ex_style) = unsafe {
            (
                GetWindowLongPtrW(h, GWL_STYLE) as isize,
                GetWindowLongPtrW(h, GWL_EXSTYLE) as isize,
            )
        };
        let removed = (WS_POPUP
            | WS_CAPTION
            | WS_THICKFRAME
            | WS_SYSMENU
            | WS_MINIMIZEBOX
            | WS_MAXIMIZEBOX
            | WS_BORDER
            | WS_DLGFRAME) as isize;
        let new_style = (style & !removed) | (WS_CHILD | WS_CLIPSIBLINGS) as isize;
        let ex_removed =
            (WS_EX_APPWINDOW | WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_DLGMODALFRAME) as isize;
        let new_ex = ex_style & !ex_removed;
        // SAFETY: as above; SetParent's null result is checked.
        unsafe {
            SetWindowLongPtrW(h, GWL_STYLE, new_style as _);
            SetWindowLongPtrW(h, GWL_EXSTYLE, new_ex as _);
            if SetParent(h, hwnd(parent)).is_null() {
                return Err(failed("SetParent"));
            }
        }
        let this = Embedded {
            window,
            style,
            ex_style,
        };
        this.place(rect, SWP_FRAMECHANGED | SWP_SHOWWINDOW)?;
        Ok(this)
    }

    /// Top-level client windows already on screen, to pass as `ignore`.
    pub fn existing_clients() -> Result<Vec<WindowId>, EmbedError> {
        scan()
    }

    /// The embedded window handle.
    pub fn window(&self) -> WindowId {
        self.window
    }

    fn place(&self, rect: Rect, extra: u32) -> Result<(), EmbedError> {
        // SAFETY: SetWindowPos on a handle; failure is reported via its result.
        let ok = unsafe {
            SetWindowPos(
                hwnd(self.window),
                std::ptr::null_mut(),
                rect.x,
                rect.y,
                rect.width as i32,
                rect.height as i32,
                extra | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        if ok == 0 {
            return Err(failed("SetWindowPos"));
        }
        Ok(())
    }

    /// Moves/resizes to follow the viewport.
    pub fn set_rect(&self, rect: Rect) -> Result<(), EmbedError> {
        self.place(rect, 0)
    }

    /// Clips the window to its rect minus `holes` (in the window's own
    /// coordinates) with a window region; empty `holes` restores it.
    pub fn set_holes(&self, holes: &[Rect]) -> Result<(), EmbedError> {
        let h = hwnd(self.window);
        if holes.is_empty() {
            // SAFETY: a null region removes the window region.
            if unsafe { SetWindowRgn(h, std::ptr::null_mut(), TRUE) } == 0 {
                return Err(failed("SetWindowRgn"));
            }
            return Ok(());
        }
        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `client` is a valid out pointer.
        if unsafe { GetClientRect(h, &mut client) } == 0 {
            return Err(failed("GetClientRect"));
        }
        let full = Rect {
            x: 0,
            y: 0,
            width: (client.right - client.left).max(0) as u32,
            height: (client.bottom - client.top).max(0) as u32,
        };
        // SAFETY: regions created here are deleted on every path except the
        // one SetWindowRgn takes ownership of on success.
        unsafe {
            let region: HRGN = CreateRectRgn(0, 0, 0, 0);
            if region.is_null() {
                return Err(failed("CreateRectRgn"));
            }
            for part in visible_parts(full, holes) {
                let piece = CreateRectRgn(
                    part.x,
                    part.y,
                    part.x + part.width as i32,
                    part.y + part.height as i32,
                );
                if piece.is_null() {
                    DeleteObject(region);
                    return Err(failed("CreateRectRgn"));
                }
                let result = CombineRgn(region, region, piece, RGN_OR);
                DeleteObject(piece);
                if result == RGN_ERROR {
                    DeleteObject(region);
                    return Err(failed("CombineRgn"));
                }
            }
            if SetWindowRgn(h, region, TRUE) == 0 {
                DeleteObject(region);
                return Err(failed("SetWindowRgn"));
            }
        }
        Ok(())
    }

    /// Hides the window, so a client about to be killed does not flash on
    /// the desktop when `Drop` hands it back.
    pub fn hide(&self) {
        // SAFETY: ShowWindow on a handle has no memory preconditions.
        unsafe { ShowWindow(hwnd(self.window), SW_HIDE) };
    }

    /// Shows the window again after `hide`, without activating it.
    pub fn show(&self) {
        // SAFETY: as in `hide`.
        unsafe { ShowWindow(hwnd(self.window), SW_SHOWNA) };
    }

    /// Gives it keyboard focus (`focus_window`).
    pub fn focus(&self) -> Result<(), EmbedError> {
        focus_window(self.window)
    }

    /// False once the window is destroyed (the client quit or crashed).
    pub fn alive(&self) -> bool {
        // SAFETY: IsWindow accepts any handle value.
        unsafe { IsWindow(hwnd(self.window)) != 0 }
    }
}

impl Drop for Embedded {
    /// If still alive, hands the window back to the desktop with its
    /// original styles so it is not destroyed with the editor's window.
    fn drop(&mut self) {
        if !self.alive() {
            return;
        }
        let h = hwnd(self.window);
        // SAFETY: plain Win32 calls on a live handle; failures are ignored.
        unsafe {
            SetParent(h, std::ptr::null_mut());
            SetWindowLongPtrW(h, GWL_STYLE, self.style as _);
            SetWindowLongPtrW(h, GWL_EXSTYLE, self.ex_style as _);
            SetWindowPos(
                h,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

/// Gives `window` keyboard focus; SetFocus only works on the caller's input
/// queue, so the client's thread is attached for the call.
fn focus_window(window: WindowId) -> Result<(), EmbedError> {
    let h = hwnd(window);
    // SAFETY: thread ids come from the OS; attach/detach are paired.
    unsafe {
        let current = GetCurrentThreadId();
        let target = GetWindowThreadProcessId(h, std::ptr::null_mut());
        if target == 0 {
            return Err(failed("GetWindowThreadProcessId"));
        }
        let attached = target != current && AttachThreadInput(current, target, TRUE) != 0;
        let focused = SetFocus(h);
        if attached {
            AttachThreadInput(current, target, FALSE);
        }
        if focused.is_null() {
            return Err(failed("SetFocus"));
        }
    }
    Ok(())
}

/// Keyboard focus for an embedded client. Windows shares the input queue
/// of a cross-process parent and child, so clicks already move focus
/// natively; this only gives the client the keyboard once at start.
pub struct FocusFollow;

impl FocusFollow {
    pub fn start(editor: WindowId, client: WindowId) -> Result<Self, EmbedError> {
        let _ = editor;
        let _ = focus_window(client);
        Ok(FocusFollow)
    }
}
