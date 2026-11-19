//! The suite's `CONFORMANCE_GESTURE` lines as real input for
//! `record_goldens`: XTest on the client's window on `$DISPLAY` (the
//! recorder's Xephyr).

use std::thread::sleep;
use std::time::Duration;

use rbx_play::embed::{is_client_class, Button, Input};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, MapState};

const STEP: Duration = Duration::from_millis(16);

pub fn perform(gesture: &str) -> Result<(), String> {
    let fail = |e: rbx_play::embed::EmbedError| format!("{e:?}");
    let window = client_window()?;
    let mut input = Input::open(window).map_err(fail)?;
    let (w, h) = input.size().map_err(fail)?;
    let (cx, cy) = (w as i32 / 2, h as i32 / 2);
    input.move_to(cx, cy).map_err(fail)?;
    let words: Vec<&str> = gesture.split_whitespace().collect();
    let number = |i: usize| {
        words
            .get(i)
            .and_then(|w| w.parse::<i32>().ok())
            .unwrap_or(0)
    };
    match words.first().copied() {
        Some("wheel") => {
            for _ in 0..number(2) {
                input.scroll(words.get(1) == Some(&"in")).map_err(fail)?;
                sleep(STEP * 4);
            }
        }
        Some("drag") => {
            let (dx, dy) = (number(1), number(2));
            input.button(Button::Right, true).map_err(fail)?;
            sleep(STEP * 2);
            let steps = 10;
            for i in 1..=steps {
                input
                    .move_to(cx + dx * i / steps, cy + dy * i / steps)
                    .map_err(fail)?;
                sleep(STEP);
            }
            sleep(STEP * 2);
            input.button(Button::Right, false).map_err(fail)?;
        }
        Some("key") => {
            let key = rbx_play::keycode::find(words.get(1).copied().unwrap_or_default())
                .ok_or("no such key")?;
            input.key(key, true).map_err(fail)?;
            sleep(STEP * 4);
            input.key(key, false).map_err(fail)?;
        }
        _ => return Err("unknown gesture".into()),
    }
    Ok(())
}

/// The biggest mapped window whose `WM_CLASS` is the client's.
fn client_window() -> Result<u32, String> {
    let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let root = conn.setup().roots[screen].root;
    let mut best: Option<(u32, u32)> = None;
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        let Ok(tree) = conn.query_tree(at).map_err(|e| e.to_string())?.reply() else {
            continue;
        };
        stack.extend(tree.children.iter().copied());
        let class = conn
            .get_property(false, at, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .map_err(|e| e.to_string())?
            .reply()
            .map(|r| r.value)
            .unwrap_or_default();
        if !is_client_class(&class) {
            continue;
        }
        let mapped = conn
            .get_window_attributes(at)
            .map_err(|e| e.to_string())?
            .reply()
            .is_ok_and(|a| a.map_state == MapState::VIEWABLE);
        let Ok(geometry) = conn.get_geometry(at).map_err(|e| e.to_string())?.reply() else {
            continue;
        };
        let area = u32::from(geometry.width) * u32::from(geometry.height);
        if mapped && best.is_none_or(|(_, a)| area > a) {
            best = Some((at, area));
        }
    }
    best.map(|(w, _)| w)
        .ok_or_else(|| "no client window".into())
}
