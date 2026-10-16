use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConnectionExt, EventMask, GrabMode, GrabStatus, ImageFormat, ImageOrder,
};
use x11rb::protocol::Event;

const UNAVAILABLE: &str = "No screen colour picker is available on this desktop.";
const ESCAPE_KEYCODE: u8 = 9;
const CROSSHAIR_GLYPH: u16 = 34;

pub(crate) fn pick(done: Box<dyn FnOnce(crate::Outcome) + Send>) {
    std::thread::spawn(move || done(pick_blocking()));
}

fn pick_blocking() -> crate::Outcome {
    if let Ok(outcome) = pick_portal() {
        return Ok(outcome);
    }
    if std::env::var_os("DISPLAY").is_some() {
        if let Some(outcome) = pick_x11() {
            return outcome;
        }
    }
    Err(UNAVAILABLE.into())
}

/// `Err` means the portal could not be used; a cancel is a successful `None`.
fn pick_portal() -> Result<Option<crate::Rgb8>, ()> {
    let result = futures_lite::future::block_on(async {
        ashpd::desktop::Color::pick().send().await?.response()
    });
    match result {
        Ok(color) => Ok(Some(crate::from_unit(
            color.red(),
            color.green(),
            color.blue(),
        ))),
        Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => Ok(None),
        Err(_) => Err(()),
    }
}

/// `None` when X11 cannot be used at all (no connection or grab refused).
fn pick_x11() -> Option<crate::Outcome> {
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = conn.setup().roots.get(screen_num)?.clone();
    let root = screen.root;

    let font = conn.generate_id().ok()?;
    conn.open_font(font, b"cursor").ok()?;
    let cursor = conn.generate_id().ok()?;
    conn.create_glyph_cursor(
        cursor,
        font,
        font,
        CROSSHAIR_GLYPH,
        CROSSHAIR_GLYPH + 1,
        0,
        0,
        0,
        0xffff,
        0xffff,
        0xffff,
    )
    .ok()?;

    let pointer = conn
        .grab_pointer(
            false,
            root,
            EventMask::BUTTON_PRESS,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            cursor,
            x11rb::CURRENT_TIME,
        )
        .ok()?
        .reply()
        .ok()?;
    if pointer.status != GrabStatus::SUCCESS {
        return None;
    }
    let keyboard_grabbed = conn
        .grab_keyboard(
            false,
            root,
            x11rb::CURRENT_TIME,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )
        .ok()
        .and_then(|c| c.reply().ok())
        .is_some_and(|r| r.status == GrabStatus::SUCCESS);

    let outcome = wait_for_pick(&conn, &screen);

    let _ = conn.ungrab_pointer(x11rb::CURRENT_TIME);
    if keyboard_grabbed {
        let _ = conn.ungrab_keyboard(x11rb::CURRENT_TIME);
    }
    let _ = conn.free_cursor(cursor);
    let _ = conn.close_font(font);
    let _ = conn.flush();
    Some(outcome)
}

fn wait_for_pick(
    conn: &impl Connection,
    screen: &x11rb::protocol::xproto::Screen,
) -> crate::Outcome {
    let failed = || "Could not read the pixel under the pointer.".to_string();
    loop {
        let event = conn.wait_for_event().map_err(|_| failed())?;
        match event {
            Event::KeyPress(key) if key.detail == ESCAPE_KEYCODE => return Ok(None),
            Event::ButtonPress(press) => match press.detail {
                3 => return Ok(None),
                1 => return read_pixel(conn, screen, press.root_x, press.root_y),
                _ => {}
            },
            _ => {}
        }
    }
}

fn read_pixel(
    conn: &impl Connection,
    screen: &x11rb::protocol::xproto::Screen,
    x: i16,
    y: i16,
) -> crate::Outcome {
    let failed = || "Could not read the pixel under the pointer.".to_string();
    let visual = screen
        .allowed_depths
        .iter()
        .flat_map(|d| d.visuals.iter())
        .find(|v| v.visual_id == screen.root_visual)
        .ok_or_else(failed)?;
    let image = conn
        .get_image(ImageFormat::Z_PIXMAP, screen.root, x, y, 1, 1, !0)
        .map_err(|_| failed())?
        .reply()
        .map_err(|_| failed())?;
    if image.data.len() < 3 {
        return Err(failed());
    }
    let mut bytes = [0u8; 4];
    let n = image.data.len().min(4);
    bytes[..n].copy_from_slice(&image.data[..n]);
    let pixel = if conn.setup().image_byte_order == ImageOrder::MSB_FIRST {
        u32::from_be_bytes(bytes) >> (8 * (4 - n as u32))
    } else {
        u32::from_le_bytes(bytes)
    };
    crate::from_masked_pixel(pixel, visual.red_mask, visual.green_mask, visual.blue_mask)
        .map(Some)
        .ok_or_else(failed)
}
