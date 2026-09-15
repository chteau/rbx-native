//! Which physical key and which wheel step mean what to the camera.

use winit::event::MouseScrollDelta;
use winit::keyboard::KeyCode;

use crate::input::CameraKey;

// Trackpads report `MouseScrollDelta::PixelDelta`, not the "one notch" line steps a
// mouse wheel sends; there's no reliable OS-independent notch size, so this is a
// documented approximation rather than a measured one.
const PIXELS_PER_WHEEL_NOTCH: f64 = 20.0;

/// Which camera key a *physical* key position drives.
///
/// Physical codes, not the logical layout-mapped key: the same positions give
/// WASD on QWERTY and ZQSD on AZERTY, matching Roblox Studio. The arrows mirror
/// the letters, and Shift is Studio's precision modifier (see [`CameraKey::Slow`]).
pub(super) fn camera_key(code: KeyCode) -> Option<CameraKey> {
    match code {
        KeyCode::KeyW | KeyCode::ArrowUp => Some(CameraKey::Forward),
        KeyCode::KeyS | KeyCode::ArrowDown => Some(CameraKey::Back),
        KeyCode::KeyA | KeyCode::ArrowLeft => Some(CameraKey::Left),
        KeyCode::KeyD | KeyCode::ArrowRight => Some(CameraKey::Right),
        KeyCode::KeyE => Some(CameraKey::Up),
        KeyCode::KeyQ => Some(CameraKey::Down),
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Some(CameraKey::Slow),
        _ => None,
    }
}

// Trackpads send fine-grained `PixelDelta`s, mice send one `LineDelta` per notch;
// scaling the former down to notch units is what lets the controller treat a
// wheel step the same way regardless of which device produced it.
pub(super) fn wheel_notches(delta: MouseScrollDelta) -> f32 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => y,
        MouseScrollDelta::PixelDelta(position) => (position.y / PIXELS_PER_WHEEL_NOTCH) as f32,
    }
}

#[cfg(test)]
#[path = "input/tests.rs"]
mod tests;
