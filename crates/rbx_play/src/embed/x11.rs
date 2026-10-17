//! The X11 back end: puts the Roblox client's window inside the editor's viewport, frameless:
//! find the client's top-level X11 window, drop its decorations, re-parent it
//! under a window the editor owns, and keep it sized to the viewport rect.
//!
//! X11 only. Sober must run under X11/XWayland for this; on a pure Wayland
//! client the window is never found and the caller leaves it standalone.
//! Flatpak gives the client its own PID namespace, so windows are found by
//! `WM_CLASS`, not `_NET_WM_PID`.

mod keys;

pub use keys::Key;

use super::{visible_parts, EmbedError, Rect};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt, InputFocus, MapState, PropMode,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Lowercased `WM_CLASS` (instance or class) substrings that identify a
/// Roblox client window: Sober and the official player under Wine.
pub const CLIENT_CLASSES: &[&str] = &["sober", "robloxplayerbeta"];

fn x11<E: ToString>(e: E) -> EmbedError {
    EmbedError::X11(e.to_string())
}

/// True when a window's `WM_CLASS` (the two NUL-separated strings, as raw
/// bytes) contains one of `CLIENT_CLASSES`, case-insensitively.
pub fn is_client_class(wm_class: &[u8]) -> bool {
    wm_class
        .to_ascii_lowercase()
        .split(|&b| b == 0)
        .any(|part| {
            let part = String::from_utf8_lossy(part);
            CLIENT_CLASSES.iter().any(|c| part.contains(c))
        })
}

/// `_MOTIF_WM_HINTS` value that removes all decorations:
/// flags = MWM_HINTS_DECORATIONS (2), functions 0, decorations 0, input mode 0, status 0.
pub fn motif_no_decorations() -> [u32; 5] {
    [2, 0, 0, 0, 0]
}

fn connect() -> Result<(RustConnection, u32), EmbedError> {
    let (conn, screen) = x11rb::connect(None).map_err(|e| EmbedError::NoDisplay(e.to_string()))?;
    let root = conn.setup().roots[screen].root;
    Ok((conn, root))
}

fn is_viewable_client(conn: &RustConnection, window: u32) -> bool {
    let viewable = conn
        .get_window_attributes(window)
        .ok()
        .and_then(|c| c.reply().ok())
        .is_some_and(|a| a.map_state == MapState::VIEWABLE);
    if !viewable {
        return false;
    }
    conn.get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
        .ok()
        .and_then(|c| c.reply().ok())
        .is_some_and(|p| is_client_class(&p.value))
}

fn children(conn: &RustConnection, window: u32) -> Vec<u32> {
    conn.query_tree(window)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|t| t.children)
        .unwrap_or_default()
}

/// Matching windows among the root's children and their children, since
/// reparenting WMs wrap each client in a frame.
fn scan(conn: &RustConnection, root: u32) -> Vec<u32> {
    let mut found = Vec::new();
    for top in children(conn, root) {
        if is_viewable_client(conn, top) {
            found.push(top);
        }
        for child in children(conn, top) {
            if is_viewable_client(conn, child) {
                found.push(child);
            }
        }
    }
    found
}

/// Withdraws a top-level window the way ICCCM 4.1.4 asks a client to: unmap
/// it, send the root a synthetic `UnmapNotify`, and wait (up to 2 s) until
/// the window manager has let go of it, i.e. removed `WM_STATE` or set it to
/// Withdrawn. Reparenting before that races the window manager, which
/// reparents a window it unmanages back to the root and manages it again
/// when it is mapped (seen with GNOME Shell). Without a window manager
/// `WM_STATE` never exists and this returns at once.
fn withdraw(conn: &RustConnection, root: u32, window: u32) -> Result<(), EmbedError> {
    use x11rb::protocol::xproto::{EventMask, UnmapNotifyEvent, UNMAP_NOTIFY_EVENT};

    conn.unmap_window(window).map_err(x11)?;
    let event = UnmapNotifyEvent {
        response_type: UNMAP_NOTIFY_EVENT,
        sequence: 0,
        event: root,
        window,
        from_configure: false,
    };
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )
    .map_err(x11)?;
    conn.flush().map_err(x11)?;
    let wm_state = conn
        .intern_atom(false, b"WM_STATE")
        .map_err(x11)?
        .reply()
        .map_err(x11)?
        .atom;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let state = conn
            .get_property(false, window, wm_state, wm_state, 0, 1)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        let withdrawn = state
            .value32()
            .and_then(|mut v| v.next())
            .is_none_or(|s| s == 0);
        if withdrawn || Instant::now() >= deadline {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn parent_of(conn: &RustConnection, window: u32) -> Option<u32> {
    conn.query_tree(window)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|t| t.parent)
}

/// A client window living inside the editor.
pub struct Embedded {
    conn: RustConnection,
    root: u32,
    window: u32,
}

impl Embedded {
    /// Opens its own X connection, polls the root window's children (and
    /// one level below, for reparenting WMs) every 100 ms until a viewable
    /// window with a client `WM_CLASS` that was not in `ignore` exists, or
    /// `timeout` passes. Then: sets `_MOTIF_WM_HINTS` to
    /// `motif_no_decorations()`, withdraws it from the window manager
    /// (`withdraw`), reparents it under `parent`
    /// at `rect`, maps it, and flushes.
    pub fn attach(
        parent: u32,
        rect: Rect,
        ignore: &[u32],
        timeout: Duration,
    ) -> Result<Self, EmbedError> {
        let (conn, root) = connect()?;
        let deadline = Instant::now() + timeout;
        let window = loop {
            if let Some(w) = scan(&conn, root).into_iter().find(|w| !ignore.contains(w)) {
                break w;
            }
            if Instant::now() >= deadline {
                return Err(EmbedError::NotFound);
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        let motif = conn
            .intern_atom(false, b"_MOTIF_WM_HINTS")
            .map_err(x11)?
            .reply()
            .map_err(x11)?
            .atom;
        conn.change_property32(
            PropMode::REPLACE,
            window,
            motif,
            motif,
            &motif_no_decorations(),
        )
        .map_err(x11)?;
        withdraw(&conn, root, window)?;
        // A window manager that was still letting go can reparent the
        // window back to the root after ours; check, and try again.
        for _ in 0..5 {
            conn.reparent_window(window, parent, rect.x as i16, rect.y as i16)
                .map_err(x11)?;
            conn.flush().map_err(x11)?;
            std::thread::sleep(Duration::from_millis(50));
            if parent_of(&conn, window) == Some(parent) {
                break;
            }
            withdraw(&conn, root, window)?;
        }
        let this = Embedded { conn, root, window };
        this.set_rect(rect)?;
        this.conn.map_window(window).map_err(x11)?;
        this.conn.flush().map_err(x11)?;
        Ok(this)
    }

    /// Top-level client windows already on screen, to pass as `ignore` so a
    /// Roblox window opened before Play is not the one grabbed.
    pub fn existing_clients() -> Result<Vec<u32>, EmbedError> {
        let (conn, root) = connect()?;
        Ok(scan(&conn, root))
    }

    /// The embedded window id.
    pub fn window(&self) -> u32 {
        self.window
    }

    /// Moves/resizes to follow the viewport.
    pub fn set_rect(&self, rect: Rect) -> Result<(), EmbedError> {
        let aux = ConfigureWindowAux::new()
            .x(rect.x)
            .y(rect.y)
            .width(rect.width)
            .height(rect.height);
        self.conn.configure_window(self.window, &aux).map_err(x11)?;
        self.conn.flush().map_err(x11)
    }

    /// Clips the window to its rect minus `holes` (rects in the window's own
    /// coordinates, i.e. relative to its top-left), with the X Shape
    /// extension's bounding region, so editor popups drawn under it show
    /// through. Empty `holes` restores the full window. Uses
    /// `visible_parts`; a hole covering everything leaves an empty shape.
    pub fn set_holes(&self, holes: &[Rect]) -> Result<(), EmbedError> {
        use x11rb::protocol::shape::{self, ConnectionExt as _};
        use x11rb::protocol::xproto::{ClipOrdering, Rectangle};

        if holes.is_empty() {
            self.conn
                .shape_mask(
                    shape::SO::SET,
                    shape::SK::BOUNDING,
                    self.window,
                    0,
                    0,
                    x11rb::NONE,
                )
                .map_err(x11)?;
        } else {
            let geometry = self
                .conn
                .get_geometry(self.window)
                .map_err(x11)?
                .reply()
                .map_err(x11)?;
            let full = Rect {
                x: 0,
                y: 0,
                width: u32::from(geometry.width),
                height: u32::from(geometry.height),
            };
            let rects: Vec<Rectangle> = visible_parts(full, holes)
                .into_iter()
                .map(|r| Rectangle {
                    x: r.x as i16,
                    y: r.y as i16,
                    width: r.width as u16,
                    height: r.height as u16,
                })
                .collect();
            self.conn
                .shape_rectangles(
                    shape::SO::SET,
                    shape::SK::BOUNDING,
                    ClipOrdering::UNSORTED,
                    self.window,
                    0,
                    0,
                    &rects,
                )
                .map_err(x11)?;
        }
        self.conn.flush().map_err(x11)
    }

    /// Unmaps the window, so a client about to be killed does not flash on
    /// the desktop when `Drop` hands it back to the root.
    pub fn hide(&self) {
        let _ = self.conn.unmap_window(self.window);
        let _ = self.conn.flush();
    }

    /// Maps the window again after `hide`, at its last rect.
    pub fn show(&self) {
        let _ = self.conn.map_window(self.window);
        let _ = self.conn.flush();
    }

    /// Gives it keyboard focus (`SetInputFocus`, revert to parent).
    pub fn focus(&self) -> Result<(), EmbedError> {
        self.conn
            .set_input_focus(InputFocus::PARENT, self.window, x11rb::CURRENT_TIME)
            .map_err(x11)?;
        self.conn.flush().map_err(x11)
    }

    /// False once the window is destroyed (the client quit or crashed);
    /// checked with `GetWindowAttributes`, which errors for a dead window.
    pub fn alive(&self) -> bool {
        self.conn
            .get_window_attributes(self.window)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some()
    }
}

impl Drop for Embedded {
    /// Gives back the keycodes typing borrowed, and if still alive,
    /// reparents the window back to the root so it is not destroyed with
    /// the editor's parent window.
    fn drop(&mut self) {
        self.return_borrowed();
        if self.alive() {
            let _ = self.conn.reparent_window(self.window, self.root, 0, 0);
            let _ = self.conn.flush();
        }
    }
}

/// Keyboard focus for an embedded client. A child window gets no focus
/// from the window manager, so this watches every pointer press (XInput2
/// raw events on the root, which every client may select) and moves X
/// focus itself: a press over the client gives it the keyboard, a press
/// anywhere else in the editor window takes it back. Runs on its own thread
/// and connection; dropping it stops the thread within ~50 ms.
pub struct FocusFollow {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// XI2's `XIAllMasterDevices`.
const ALL_MASTER_DEVICES: u16 = 1;

impl FocusFollow {
    /// Starts following for `client`, embedded in the editor's top-level
    /// `editor` window, and gives `client` the keyboard right away.
    pub fn start(editor: u32, client: u32) -> Result<Self, EmbedError> {
        use x11rb::protocol::xinput::{self, ConnectionExt as _};

        let (conn, root) = connect()?;
        conn.xinput_xi_query_version(2, 2)
            .map_err(x11)?
            .reply()
            .map_err(|e| EmbedError::X11(format!("XInput 2.2 is not available: {e}")))?;
        conn.xinput_xi_select_events(
            root,
            &[xinput::EventMask {
                deviceid: ALL_MASTER_DEVICES,
                mask: vec![xinput::XIEventMask::RAW_BUTTON_PRESS],
            }],
        )
        .map_err(x11)?;
        conn.set_input_focus(InputFocus::PARENT, client, x11rb::CURRENT_TIME)
            .map_err(x11)?;
        conn.flush().map_err(x11)?;

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("embed-focus".into())
            .spawn(move || {
                use std::sync::atomic::Ordering;
                while !stopping.load(Ordering::Relaxed) {
                    let mut pressed = false;
                    while let Ok(Some(event)) = conn.poll_for_event() {
                        if matches!(event, x11rb::protocol::Event::XinputRawButtonPress(_)) {
                            pressed = true;
                        }
                    }
                    if pressed {
                        // After the window manager's own click-to-focus, so
                        // ours is the focus change that sticks.
                        std::thread::sleep(Duration::from_millis(30));
                        let _ = route_focus(&conn, root, editor, client);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            })
            .map_err(|e| EmbedError::X11(e.to_string()))?;
        Ok(FocusFollow {
            stop,
            thread: Some(thread),
        })
    }
}

/// Which window is under the pointer decides: the client (or anything
/// inside it) takes focus; elsewhere in the editor's top-level, the editor.
fn route_focus(
    conn: &RustConnection,
    root: u32,
    editor: u32,
    client: u32,
) -> Result<(), EmbedError> {
    let mut window = root;
    let mut path = Vec::new();
    loop {
        let reply = conn
            .query_pointer(window)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        if reply.child == x11rb::NONE {
            break;
        }
        window = reply.child;
        path.push(window);
    }
    let target = if path.contains(&client) {
        client
    } else if path.contains(&editor) || path.iter().any(|&w| is_ancestor(conn, w, editor)) {
        editor
    } else {
        return Ok(());
    };
    conn.set_input_focus(InputFocus::PARENT, target, x11rb::CURRENT_TIME)
        .map_err(x11)?;
    conn.flush().map_err(x11)
}

/// Whether `window` is `editor` or contains it (a WM frame around it).
fn is_ancestor(conn: &RustConnection, window: u32, editor: u32) -> bool {
    let mut current = editor;
    for _ in 0..8 {
        if current == window {
            return true;
        }
        match conn.query_tree(current).ok().and_then(|c| c.reply().ok()) {
            Some(tree) if tree.parent != tree.root && tree.parent != x11rb::NONE => {
                current = tree.parent;
            }
            Some(tree) => return tree.parent == window,
            None => return false,
        }
    }
    false
}

impl Drop for FocusFollow {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
