use super::*;
use winit::dpi::PhysicalPosition;

#[test]
fn a_line_delta_notch_is_used_as_is() {
    assert_eq!(wheel_notches(MouseScrollDelta::LineDelta(0.0, 1.0)), 1.0);
    assert_eq!(wheel_notches(MouseScrollDelta::LineDelta(0.0, -2.5)), -2.5);
}

#[test]
fn a_pixel_delta_is_scaled_down_to_notch_units() {
    let up = wheel_notches(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
        0.0,
        PIXELS_PER_WHEEL_NOTCH,
    )));
    assert!((up - 1.0).abs() < 1e-9);

    let double = wheel_notches(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
        0.0,
        2.0 * PIXELS_PER_WHEEL_NOTCH,
    )));
    assert!((double - 2.0).abs() < 1e-9);
}

#[test]
fn line_and_pixel_deltas_agree_on_direction() {
    let line = wheel_notches(MouseScrollDelta::LineDelta(0.0, 1.0));
    let pixel = wheel_notches(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
        0.0,
        PIXELS_PER_WHEEL_NOTCH,
    )));
    assert_eq!(line.signum(), pixel.signum());
}

// The AZERTY promise, stated in physical terms: the key positions W/A/S/D sit on
// are the ones that move, whatever letter the layout prints on them.
#[test]
fn the_wasd_positions_and_the_arrows_drive_the_same_keys() {
    for (code, expected) in [
        (KeyCode::KeyW, CameraKey::Forward),
        (KeyCode::ArrowUp, CameraKey::Forward),
        (KeyCode::KeyS, CameraKey::Back),
        (KeyCode::ArrowDown, CameraKey::Back),
        (KeyCode::KeyA, CameraKey::Left),
        (KeyCode::ArrowLeft, CameraKey::Left),
        (KeyCode::KeyD, CameraKey::Right),
        (KeyCode::ArrowRight, CameraKey::Right),
        (KeyCode::KeyE, CameraKey::Up),
        (KeyCode::KeyQ, CameraKey::Down),
        (KeyCode::ShiftLeft, CameraKey::Slow),
        (KeyCode::ShiftRight, CameraKey::Slow),
    ] {
        assert_eq!(camera_key(code), Some(expected), "{code:?}");
    }
}

#[test]
fn an_unrelated_key_moves_nothing() {
    assert_eq!(camera_key(KeyCode::KeyP), None);
    assert_eq!(camera_key(KeyCode::Space), None);
}
