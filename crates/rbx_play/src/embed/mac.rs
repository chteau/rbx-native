//! The macOS back end: keeps the Roblox player's own window pinned over
//! the editor's viewport.
//!
//! macOS has no cross-process window parenting, so the player keeps its
//! window (and title bar) and is moved instead: found in the window
//! server's list by its owner (`pin::is_client`), placed with the
//! Accessibility API at the viewport's rect in global points, raised over
//! the editor when the editor is clicked to the front, and hidden while the
//! editor is minimised, hidden or on another Space, while another document
//! covers the 3D view, and while an editor popup overlaps it (a separate
//! window cannot be cut around a popup). The editor's window is passed as
//! its window server number.
//!
//! All of it needs the Accessibility permission; without it `attach`
//! fails with `EmbedError::NoAccess` and the player stays where it is.
//!
//! What it sees and does goes to stderr as `mac embed:` lines, once per
//! change, since nobody can watch the window server from a bug report.

mod capture;
mod events;
mod ffi;
mod input;

pub use input::Input;

use std::cell::{Cell, RefCell};

use objc2_app_kit::NSRunningApplication;
use std::time::{Duration, Instant};

use super::pin::{self, Frame};
use super::{EmbedError, Rect, WindowId};
use ffi::{Ax, WindowInfo};

/// How long the player may show no window before it counts as closed: its
/// loading window gives way to the game's.
const GONE_GRACE: Duration = Duration::from_secs(5);

fn failed(call: &str) -> EmbedError {
    EmbedError::X11(format!("{call} failed"))
}

/// This process's id, as the window server lists owners.
fn own_pid() -> i32 {
    i32::try_from(std::process::id()).unwrap_or(-1)
}

/// Hides or shows the app `pid` the way Cmd+H does, through AppKit, which
/// needs no permission; the Accessibility attribute is the fallback.
fn set_app_hidden(pid: i32, app: &Ax, hidden: bool) -> bool {
    let done = NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(
        |running| match hidden {
            true => running.hide() || running.isHidden(),
            false => running.unhide() || !running.isHidden(),
        },
    );
    done || app.set_bool("AXHidden", hidden)
}

/// The player's game windows on screen, front first.
fn scan() -> Vec<WindowInfo> {
    ffi::windows(ffi::ON_SCREEN, 0)
        .into_iter()
        .filter(|w| pin::is_client(&w.owner, w.layer, w.frame))
        .collect()
}

/// The player window being pinned, and its Accessibility handles.
struct Client {
    id: WindowId,
    pid: i32,
    app: Ax,
    window: Option<Ax>,
}

impl Client {
    fn adopt(info: &WindowInfo) -> Result<Self, EmbedError> {
        let app = Ax::app(info.pid).ok_or_else(|| failed("AXUIElementCreateApplication"))?;
        let window = app.window(info.id);
        Ok(Client {
            id: info.id,
            pid: info.pid,
            app,
            window,
        })
    }
}

/// A player window pinned over the editor's viewport.
pub struct Embedded {
    parent: WindowId,
    client: RefCell<Client>,
    /// `hide` was called (another document covers the 3D view).
    hidden: Cell<bool>,
    /// An editor popup overlaps the viewport.
    holed: Cell<bool>,
    /// The editor's window is not on screen.
    offscreen: Cell<bool>,
    /// Whether the player app is shown, as last set.
    shown: Cell<bool>,
    missing_since: Cell<Option<Instant>>,
    /// The last `mac embed:` line written, so each is written once.
    said: RefCell<String>,
}

impl Embedded {
    /// Polls every 100 ms until a player window not in `ignore` is on
    /// screen, or `timeout` passes; then pins it at `rect` in `parent`.
    /// Fails at once, after macOS's prompt, without the Accessibility
    /// permission.
    pub fn attach(
        parent: WindowId,
        rect: Rect,
        ignore: &[WindowId],
        timeout: Duration,
    ) -> Result<Self, EmbedError> {
        if !ffi::trusted_or_prompt() {
            return Err(EmbedError::NoAccess);
        }
        let deadline = Instant::now() + timeout;
        let info = loop {
            if let Some(w) = scan().into_iter().find(|w| !ignore.contains(&w.id)) {
                break w;
            }
            if Instant::now() >= deadline {
                return Err(EmbedError::NotFound);
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let this = Embedded {
            parent,
            client: RefCell::new(Client::adopt(&info)?),
            hidden: Cell::new(false),
            holed: Cell::new(false),
            offscreen: Cell::new(false),
            shown: Cell::new(true),
            missing_since: Cell::new(None),
            said: RefCell::new(String::new()),
        };
        this.say(format!("pinning {} ({}) of pid {}", info.id, info.owner, info.pid));
        this.set_rect(rect)?;
        Ok(this)
    }

    /// Nothing to ignore: macOS runs one player at a time, and a Play
    /// started while it runs hands the game to that same player.
    pub fn existing_clients() -> Result<Vec<WindowId>, EmbedError> {
        Ok(Vec::new())
    }

    /// The player window's window server number.
    pub fn window(&self) -> WindowId {
        self.client.borrow().id
    }

    /// Pins the player at `rect` and keeps it over the editor. The editor's
    /// window can move without `rect` changing, so this is meant to be
    /// called on every poll; it only moves or raises what is out of place.
    pub fn set_rect(&self, rect: Rect) -> Result<(), EmbedError> {
        let editor = ffi::window(self.parent).ok_or_else(|| failed("finding the editor window"))?;
        // Minimised, on another Space, or the app hidden with Cmd+H: the
        // window server lists the editor's window as off screen.
        let hidden_app = NSRunningApplication::currentApplication().isHidden();
        self.offscreen.set(!editor.onscreen || hidden_app);
        self.sync_shown();
        if !self.shown.get() {
            return Ok(());
        }
        let target = pin::screen_frame(editor.frame, rect, ffi::display_scale(editor.frame));
        let mut client = self.client.borrow_mut();
        if client.window.is_none() {
            client.window = client.app.window(client.id);
        }
        let window = client.window.as_ref().ok_or_else(|| failed("AXWindows"))?;
        let now = ffi::window(client.id).map(|w| w.frame);
        let placed = now.is_some_and(|frame| frame.near(target));
        if !placed && !window.set_frame(target) {
            // The element may be stale (the player rebuilt its window).
            client.window = None;
            drop(client);
            self.say("moving the player failed (AXPosition/AXSize)".to_owned());
            return Err(failed("AXPosition/AXSize"));
        }
        if let Some(frame) = ffi::window(client.id).map(|w| w.frame) {
            if frame.height > target.height + 1.0 || frame.width > target.width + 1.0 {
                // The player has a minimum size; it overflows the view.
                let (w, h) = (frame.width, frame.height);
                let (tw, th) = (target.width, target.height);
                let id = client.id;
                drop(client);
                self.say(format!("player {id} kept {w}x{h} for a {tw}x{th} view"));
                client = self.client.borrow_mut();
            }
        }
        let order: Vec<(u32, i32)> = ffi::windows(ffi::ON_SCREEN, 0)
            .into_iter()
            .filter(|w| w.layer == 0)
            .map(|w| (w.id, w.pid))
            .collect();
        if pin::needs_raise(&order, own_pid(), self.parent, client.id) {
            let raised = client.window.as_ref().is_some_and(Ax::raise);
            let id = client.id;
            drop(client);
            let front: Vec<u32> = order.iter().take(4).map(|&(w, _)| w).collect();
            self.say(format!(
                "raising player {id} over editor {} (front {front:?}): {}",
                self.parent,
                match raised {
                    true => "ok",
                    false => "AXRaise refused",
                }
            ));
        }
        Ok(())
    }

    /// Writes `line` to stderr unless it was the last one written.
    fn say(&self, line: String) {
        let mut said = self.said.borrow_mut();
        if *said != line {
            eprintln!("mac embed: {line}");
            *said = line;
        }
    }

    /// A separate window cannot be clipped around the editor's popups:
    /// the player is hidden while any overlaps it.
    pub fn set_holes(&self, holes: &[Rect]) -> Result<(), EmbedError> {
        self.holed.set(!holes.is_empty());
        self.sync_shown();
        Ok(())
    }

    /// Hides the player app (another document covers the 3D view).
    pub fn hide(&self) {
        self.hidden.set(true);
        self.sync_shown();
    }

    /// Shows the player app again after `hide`, without activating it.
    pub fn show(&self) {
        self.hidden.set(false);
        self.sync_shown();
    }

    fn sync_shown(&self) {
        let want = !(self.hidden.get() || self.holed.get() || self.offscreen.get());
        if want == self.shown.get() {
            return;
        }
        let done = {
            let client = self.client.borrow();
            set_app_hidden(client.pid, &client.app, !want)
        };
        if done {
            self.shown.set(want);
        }
        let why = match (self.hidden.get(), self.holed.get(), self.offscreen.get()) {
            (true, ..) => "another document is in front",
            (_, true, _) => "a popup overlaps the view",
            (.., true) => "the editor is minimised, hidden or on another Space",
            _ => "the 3D view is back",
        };
        self.say(format!(
            "{} the player ({why}){}",
            match want {
                true => "showing",
                false => "hiding",
            },
            match done {
                true => "",
                false => ": refused",
            }
        ));
    }

    /// Activates the player app, which gives it the keyboard.
    pub fn focus(&self) -> Result<(), EmbedError> {
        focus_window(self.window())
    }

    /// False once the player has had no window for `GONE_GRACE`. A new
    /// player window (its loading window replaced, or a relaunch) is
    /// adopted instead.
    pub fn alive(&self) -> bool {
        if ffi::window(self.window()).is_some() {
            self.missing_since.set(None);
            return true;
        }
        if let Some(next) = scan().first() {
            if let Ok(client) = Client::adopt(next) {
                *self.client.borrow_mut() = client;
                self.missing_since.set(None);
                return true;
            }
        }
        let since = self.missing_since.get().unwrap_or_else(Instant::now);
        self.missing_since.set(Some(since));
        since.elapsed() < GONE_GRACE
    }
}

impl Drop for Embedded {
    /// Shows the player again if it was hidden: macOS keeps it running
    /// after Play stops, in its own window as before.
    fn drop(&mut self) {
        if !self.shown.get() {
            let client = self.client.borrow();
            set_app_hidden(client.pid, &client.app, false);
        }
    }
}

/// Activates the app owning `window`.
fn focus_window(window: WindowId) -> Result<(), EmbedError> {
    let info = ffi::window(window).ok_or(EmbedError::NotFound)?;
    let app = Ax::app(info.pid).ok_or_else(|| failed("AXUIElementCreateApplication"))?;
    if !app.set_bool("AXFrontmost", true) {
        return Err(failed("AXFrontmost"));
    }
    Ok(())
}

/// Keyboard focus for a pinned player: it is an app of its own, so a click
/// on it activates it natively; this only gives it the keyboard at start.
pub struct FocusFollow;

impl FocusFollow {
    pub fn start(editor: WindowId, client: WindowId) -> Result<Self, EmbedError> {
        let _ = editor;
        let _ = focus_window(client);
        Ok(FocusFollow)
    }

    /// macOS moves focus natively; nothing to keep.
    pub fn keep_client_focus_in(&self, rect: Option<Rect>) {
        let _ = rect;
    }

    /// Not followed on macOS.
    pub fn pointer_inside(&self) -> Option<bool> {
        None
    }
}

/// The player window's frame and its display's scale.
fn frame_and_scale(window: WindowId) -> Result<(Frame, f64), EmbedError> {
    let info = ffi::window(window).ok_or(EmbedError::NotFound)?;
    Ok((info.frame, ffi::display_scale(info.frame)))
}
