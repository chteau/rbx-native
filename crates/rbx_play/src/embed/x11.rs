//! The X11 back end: puts the Roblox client's window inside the editor's viewport, frameless:
//! find the client's top-level X11 window, drop its decorations, re-parent it
//! under a window the editor owns, and keep it sized to the viewport rect.
//!
//! X11 only. Sober must run under X11/XWayland for this; on a pure Wayland
//! client the window is never found and the caller leaves it standalone.
//! Flatpak gives the client its own PID namespace, so windows are found by
//! `WM_CLASS`, not `_NET_WM_PID`, among those created after Play started
//! ([`Before`]).

mod focus;
mod input;
mod keys;

pub use input::Input;
pub use keys::Key;

use super::{visible_parts, EmbedError, Rect};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConfigureWindowAux, ConnectionExt, EventMask, InputFocus,
    MapState, PropMode,
};
use x11rb::protocol::Event;
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

/// Gives the client the keyboard however the desktop is arranged: the X
/// focus, which XTest's keys go to, and an `EnterNotify`. Sober drops key
/// presses (not releases) while it believes the pointer is elsewhere, as it
/// is whenever another window covers the editor or is active; a synthetic
/// crossing is enough for it (measured), so the pointer is never moved.
/// Placed where the pointer is when that is over the window, else in its
/// middle.
fn claim_keyboard(conn: &RustConnection, root: u32, window: u32) -> Result<(), EmbedError> {
    use x11rb::protocol::xproto::{EnterNotifyEvent, NotifyDetail, NotifyMode, ENTER_NOTIFY_EVENT};

    conn.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
        .map_err(x11)?;
    let size = conn
        .get_geometry(window)
        .map_err(x11)?
        .reply()
        .map_err(x11)?;
    let pointer = conn
        .query_pointer(window)
        .map_err(x11)?
        .reply()
        .map_err(x11)?;
    let within = |at: i16, extent: u16| {
        if (0..i32::from(extent)).contains(&i32::from(at)) {
            at
        } else {
            i16::try_from(extent / 2).unwrap_or(0)
        }
    };
    let enter = EnterNotifyEvent {
        response_type: ENTER_NOTIFY_EVENT,
        detail: NotifyDetail::NONLINEAR,
        sequence: 0,
        time: x11rb::CURRENT_TIME,
        root,
        event: window,
        child: x11rb::NONE,
        root_x: pointer.root_x,
        root_y: pointer.root_y,
        event_x: within(pointer.win_x, size.width),
        event_y: within(pointer.win_y, size.height),
        state: 0u16.into(),
        mode: NotifyMode::NORMAL,
        // `same-screen` and `focus`.
        same_screen_focus: 3,
    };
    conn.send_event(false, window, EventMask::ENTER_WINDOW, enter)
        .map_err(x11)?;
    Ok(())
}

fn parent_of(conn: &RustConnection, window: u32) -> Option<u32> {
    conn.query_tree(window)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|t| t.parent)
}

/// The screen as it was before the client started, so only a window
/// created since counts as the new client's. Window ids cannot tell: the X
/// server hands a closed client's ids to the next one (every Sober run here
/// got the same window id), so a Sober window still on screen when Play
/// started, one closing down or left over, used to put the new client's id
/// on the ignore list and the 90 s wait ran out with the game open.
/// Default: no watch, any client window counts.
#[derive(Default)]
pub struct Before {
    watch: Option<(RustConnection, u32)>,
}

impl Before {
    /// Every window created as a top-level since the watch began and not
    /// destroyed since, added to `fresh`.
    fn note_created(&self, fresh: &mut std::collections::HashSet<u32>) {
        let Some((conn, root)) = &self.watch else {
            return;
        };
        while let Ok(Some(event)) = conn.poll_for_event() {
            match event {
                Event::CreateNotify(e) if e.parent == *root => {
                    fresh.insert(e.window);
                }
                Event::DestroyNotify(e) => {
                    fresh.remove(&e.window);
                }
                _ => {}
            }
        }
    }

    fn is_new(&self, window: u32, fresh: &std::collections::HashSet<u32>) -> bool {
        self.watch.is_none() || fresh.contains(&window)
    }
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
    /// window with a client `WM_CLASS` created after `before` exists, or
    /// `timeout` passes. Then: sets `_MOTIF_WM_HINTS` to
    /// `motif_no_decorations()`, withdraws it from the window manager
    /// (`withdraw`), reparents it under `parent`
    /// at `rect`, maps it, and flushes.
    pub fn attach(
        parent: u32,
        rect: Rect,
        before: &Before,
        timeout: Duration,
    ) -> Result<Self, EmbedError> {
        let (conn, root) = connect()?;
        let deadline = Instant::now() + timeout;
        let mut fresh = std::collections::HashSet::new();
        let window = loop {
            before.note_created(&mut fresh);
            if let Some(w) = scan(&conn, root)
                .into_iter()
                .find(|&w| before.is_new(w, &fresh))
            {
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

    /// Starts watching for new top-level windows, before the client is
    /// started, so a Roblox window opened before Play is not the one
    /// grabbed ([`Before`]).
    pub fn existing_clients() -> Result<Before, EmbedError> {
        let (conn, root) = connect()?;
        conn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY),
        )
        .map_err(x11)?;
        conn.sync().map_err(x11)?;
        Ok(Before {
            watch: Some((conn, root)),
        })
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

    /// The window's whole contents as `(width, height, RGBA)`.
    pub fn capture(&self) -> Result<(u32, u32, Vec<u8>), EmbedError> {
        use x11rb::protocol::xproto::ImageFormat;

        let g = self
            .conn
            .get_geometry(self.window)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        let (w, h) = (u32::from(g.width), u32::from(g.height));
        let image = self
            .conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.window,
                0,
                0,
                g.width,
                g.height,
                !0,
            )
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        if image.depth != 24 && image.depth != 32 {
            return Err(EmbedError::X11(format!(
                "unsupported window depth {}",
                image.depth
            )));
        }
        if image.data.len() != (w as usize) * (h as usize) * 4 {
            return Err(EmbedError::X11("unexpected image size".into()));
        }
        Ok((w, h, super::bgrx_to_rgba(&image.data)))
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
/// anywhere else in the editor window takes it back. It also follows raw
/// motion, to tell whether the pointer is over the client
/// (`pointer_inside`): Sober reads the wheel wherever the pointer is.
/// Runs on its own thread and connection; dropping it stops the thread
/// within ~50 ms.
pub struct FocusFollow {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// `POINTER_*`.
    pointer: std::sync::Arc<std::sync::atomic::AtomicU8>,
    thread: Option<std::thread::JoinHandle<()>>,
}

const POINTER_UNKNOWN: u8 = 0;
const POINTER_INSIDE: u8 = 1;
const POINTER_OUTSIDE: u8 = 2;

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
                mask: vec![xinput::XIEventMask::RAW_BUTTON_PRESS | xinput::XIEventMask::RAW_MOTION],
            }],
        )
        .map_err(x11)?;
        conn.set_input_focus(InputFocus::PARENT, client, x11rb::CURRENT_TIME)
            .map_err(x11)?;
        conn.flush().map_err(x11)?;

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = stop.clone();
        let pointer = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(POINTER_UNKNOWN));
        let pointing = pointer.clone();
        let thread = std::thread::Builder::new()
            .name("embed-focus".into())
            .spawn(move || {
                use std::sync::atomic::Ordering;
                let mut moved = true;
                while !stopping.load(Ordering::Relaxed) {
                    let mut pressed = false;
                    while let Ok(Some(event)) = conn.poll_for_event() {
                        match event {
                            x11rb::protocol::Event::XinputRawButtonPress(_) => pressed = true,
                            x11rb::protocol::Event::XinputRawMotion(_) => moved = true,
                            _ => {}
                        }
                    }
                    if moved || pressed {
                        moved = false;
                        if let Ok(path) = pointer_path(&conn, root) {
                            let inside = path.contains(&client);
                            pointing.store(
                                if inside {
                                    POINTER_INSIDE
                                } else {
                                    POINTER_OUTSIDE
                                },
                                Ordering::Relaxed,
                            );
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
            pointer,
            thread: Some(thread),
        })
    }

    /// Whether the pointer was over the client at its last move; `None`
    /// before the first reading.
    pub fn pointer_inside(&self) -> Option<bool> {
        match self.pointer.load(std::sync::atomic::Ordering::Relaxed) {
            POINTER_INSIDE => Some(true),
            POINTER_OUTSIDE => Some(false),
            _ => None,
        }
    }
}

/// The windows under the pointer, outermost first, below `root`.
fn pointer_path(conn: &RustConnection, root: u32) -> Result<Vec<u32>, EmbedError> {
    let mut window = root;
    let mut path = Vec::new();
    loop {
        let reply = conn
            .query_pointer(window)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        if reply.child == x11rb::NONE {
            return Ok(path);
        }
        window = reply.child;
        path.push(window);
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
    let path = pointer_path(conn, root)?;
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
