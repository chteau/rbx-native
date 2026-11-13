//! The X11 side: which compositor runs, and the blur hint KWin and picom
//! read.

use std::sync::OnceLock;

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode, Window};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use super::BlurSupport;

/// One connection for detection and hints, opened on first use. The
/// backdrop worker has its own, since it listens for events.
fn shared() -> Option<&'static (RustConnection, usize)> {
    static CONNECTION: OnceLock<Option<(RustConnection, usize)>> = OnceLock::new();
    CONNECTION
        .get_or_init(|| x11rb::connect(None).ok())
        .as_ref()
}

/// GPUI's own choice of back end: Wayland whenever `WAYLAND_DISPLAY` names
/// one.
fn on_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty())
}

pub(super) fn detect() -> BlurSupport {
    if on_wayland() {
        // GPUI asks for KDE's blur protocol, which only KWin offers.
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        return match desktop.to_ascii_lowercase().contains("kde") {
            true => BlurSupport::Native,
            false => BlurSupport::None,
        };
    }
    let Some((conn, screen)) = shared() else {
        return BlurSupport::None;
    };
    let root = conn.setup().roots[*screen].root;
    let wm = supporting_wm(conn, root).unwrap_or_default();
    let compositor = compositor_names(conn, *screen);
    let support = classify(&wm, compositor.as_deref());
    if support == BlurSupport::Drawn && !super::capture::extensions_present(conn) {
        eprintln!("rbxstudio: no Composite, Render or Damage extension, blur shows see-through");
        return BlurSupport::None;
    }
    support
}

/// The verdict from the window manager's name and the names the running
/// compositor goes by (`None`: nothing composites, so nothing shows through
/// at all).
pub(super) fn classify(wm: &str, compositor: Option<&[String]>) -> BlurSupport {
    let Some(compositor) = compositor else {
        return BlurSupport::None;
    };
    let wm = wm.to_ascii_lowercase();
    let names = || {
        compositor
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .chain(std::iter::once(wm.clone()))
    };
    if names().any(|name| {
        ["kwin", "picom", "compton"]
            .iter()
            .any(|n| name.contains(n))
    }) {
        BlurSupport::Native
    } else if names().any(|name| name.contains("gnome shell") || name.contains("mutter")) {
        BlurSupport::Drawn
    } else {
        BlurSupport::None
    }
}

fn atom(conn: &RustConnection, name: &str) -> Option<u32> {
    Some(
        conn.intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom,
    )
}

fn text_property(conn: &RustConnection, window: Window, name: &str) -> Option<String> {
    let property = atom(conn, name)?;
    let reply = conn
        .get_property(false, window, property, AtomEnum::ANY, 0, 256)
        .ok()?
        .reply()
        .ok()?;
    // WM_CLASS is two NUL-separated strings; both are worth matching.
    let text = String::from_utf8_lossy(&reply.value).replace('\0', " ");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn supporting_wm(conn: &RustConnection, root: Window) -> Option<String> {
    let check = atom(conn, "_NET_SUPPORTING_WM_CHECK")?;
    let reply = conn
        .get_property(false, root, check, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let window = reply.value32()?.next()?;
    text_property(conn, window, "_NET_WM_NAME").or_else(|| text_property(conn, window, "WM_NAME"))
}

/// Everything the `_NET_WM_CM_S<screen>` owner calls itself: its names, its
/// class and its process's name.
fn compositor_names(conn: &RustConnection, screen: usize) -> Option<Vec<String>> {
    let selection = atom(conn, &format!("_NET_WM_CM_S{screen}"))?;
    let owner = conn
        .get_selection_owner(selection)
        .ok()?
        .reply()
        .ok()?
        .owner;
    if owner == x11rb::NONE {
        return None;
    }
    let mut names: Vec<String> = ["_NET_WM_NAME", "WM_NAME", "WM_CLASS"]
        .iter()
        .filter_map(|name| text_property(conn, owner, name))
        .collect();
    let pid = atom(conn, "_NET_WM_PID").and_then(|property| {
        let reply = conn
            .get_property(false, owner, property, AtomEnum::CARDINAL, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let pid = reply.value32()?.next();
        pid
    });
    if let Some(comm) =
        pid.and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
    {
        names.push(comm.trim().to_owned());
    }
    Some(names)
}

/// `_KDE_NET_WM_BLUR_BEHIND_REGION`, empty for the whole window, while the
/// theme is blurred; removed otherwise.
pub(super) fn set_hint(window: Window, blurred: bool) {
    let Some((conn, _)) = shared() else {
        return;
    };
    let Some(property) = atom(conn, "_KDE_NET_WM_BLUR_BEHIND_REGION") else {
        return;
    };
    let _ = match blurred {
        true => {
            conn.change_property32(PropMode::REPLACE, window, property, AtomEnum::CARDINAL, &[])
        }
        false => conn.delete_property(window, property),
    };
    let _ = conn.flush();
}
