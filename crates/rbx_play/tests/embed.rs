#![cfg(target_os = "linux")]

use rbx_play::embed::{is_client_class, motif_no_decorations, visible_parts, Embedded, Rect};

fn r(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn area(rects: &[Rect]) -> u64 {
    rects
        .iter()
        .map(|r| u64::from(r.width) * u64::from(r.height))
        .sum()
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width as i32
        && b.x < a.x + a.width as i32
        && a.y < b.y + b.height as i32
        && b.y < a.y + a.height as i32
}

#[test]
fn no_holes_keeps_full() {
    assert_eq!(visible_parts(r(0, 0, 100, 50), &[]), vec![r(0, 0, 100, 50)]);
}

#[test]
fn hole_outside_keeps_full() {
    let full = r(0, 0, 100, 50);
    assert_eq!(visible_parts(full, &[r(200, 200, 10, 10)]), vec![full]);
}

#[test]
fn hole_covering_all_leaves_nothing() {
    assert!(visible_parts(r(0, 0, 100, 50), &[r(-10, -10, 200, 200)]).is_empty());
}

#[test]
fn centre_hole_makes_four_parts() {
    let hole = r(40, 20, 20, 10);
    let parts = visible_parts(r(0, 0, 100, 50), &[hole]);
    assert_eq!(parts.len(), 4);
    assert_eq!(area(&parts), 5000 - 200);
    assert!(parts.iter().all(|p| !overlaps(*p, hole)));
}

#[test]
fn overlapping_holes_total_area() {
    let parts = visible_parts(r(0, 0, 100, 100), &[r(10, 10, 30, 30), r(20, 20, 30, 30)]);
    assert_eq!(area(&parts), 10000 - (900 + 900 - 400));
    for (i, a) in parts.iter().enumerate() {
        for b in &parts[i + 1..] {
            assert!(!overlaps(*a, *b));
        }
    }
}

#[test]
fn sober_class_matches() {
    assert!(is_client_class(b"sober\0Sober\0"));
}

#[test]
fn wine_player_class_matches() {
    assert!(is_client_class(b"Wine\0RobloxPlayerBeta.exe\0"));
}

#[test]
fn other_class_does_not_match() {
    assert!(!is_client_class(b"firefox\0Firefox\0"));
}

#[test]
fn empty_class_does_not_match() {
    assert!(!is_client_class(b""));
}

#[test]
fn motif_hints_remove_decorations() {
    assert_eq!(motif_no_decorations(), [2, 0, 0, 0, 0]);
}

/// A mapped top-level window with Sober's `WM_CLASS`, on its own
/// connection (as the real client is), and that connection.
fn sober_window() -> (x11rb::rust_connection::RustConnection, u32) {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt, CreateWindowAux, PropMode, WindowClass,
    };
    use x11rb::wrapper::ConnectionExt as _;

    let (conn, screen) = x11rb::connect(None).unwrap();
    let root = conn.setup().roots[screen].root;
    let window = conn.generate_id().unwrap();
    conn.create_window(
        0,
        window,
        root,
        0,
        0,
        64,
        48,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new(),
    )
    .unwrap();
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"sober\0org.vinegarhq.Sober\0",
    )
    .unwrap();
    conn.map_window(window).unwrap();
    conn.sync().unwrap();
    (conn, window)
}

/// A Sober window already on screen when Play starts is never taken for
/// the new client's, and the new one is found whatever its id.
#[test]
#[ignore = "needs a running X display ($DISPLAY) with no window manager"]
fn only_a_window_created_after_the_watch_is_attached() {
    let (_old_conn, old) = sober_window();
    let (_parent_conn, parent) = sober_window();
    let before = Embedded::existing_clients().unwrap();
    let missed = Embedded::attach(
        parent,
        r(0, 0, 32, 24),
        &before,
        std::time::Duration::from_millis(300),
    );
    assert!(missed.is_err(), "a window from before Play was taken");
    let (_new_conn, new) = sober_window();
    let embedded = Embedded::attach(
        parent,
        r(0, 0, 32, 24),
        &before,
        std::time::Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(embedded.window(), new);
    assert_ne!(embedded.window(), old);
}
