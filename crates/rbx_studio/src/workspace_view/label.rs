//! What the view reads out about itself: the quality level and frame rate the
//! Viewport dock lists (see `shell::viewport_dock`), and the flight speed the
//! viewport shows for a moment after the wheel changes it.
//!
//! Nothing here stays on the 3D view. A persistent label in its corner was
//! one more thing sat over the scene being edited, so the level and the rate
//! live in a dock instead; only the speed, which is gone again in a second
//! and is feedback on the gesture in progress, is still drawn over it.

use gpui_kit::SharedString;
use rbx_viewer::QualityLevel;

/// The quality level the renderer is drawing at.
///
/// `Automatic` says so rather than showing the bare level: a level that moves
/// on its own is otherwise read as the renderer misbehaving.
pub(super) fn quality(mode: QualityLevel, level: u8) -> SharedString {
    match mode {
        QualityLevel::Automatic => format!("Auto\u{b7}Q{level}"),
        QualityLevel::Level(_) => format!("Q{level}"),
    }
    .into()
}

/// The last second's frame rate and its frame time, or a placeholder while
/// the first second since sampling started is still being counted — `0.0` is
/// "not measured yet", never a real rate.
pub(super) fn frame_rate(fps: f32) -> SharedString {
    if fps > 0.0 {
        // Formatted by `rbx_viewer::fps_readout`, which `rbxview`'s own title
        // bar reads the same way; the frame time is exactly 1/fps over the
        // same window, not a separate approximation.
        rbx_viewer::fps_readout(fps).into()
    } else {
        "Measuring\u{2026}".into()
    }
}

/// The flight speed, while its moment on screen lasts.
pub(super) fn speed(speed: f32) -> SharedString {
    format!("{} studs/s", speed.round() as i64).into()
}

#[cfg(test)]
mod tests {
    use rbx_viewer::QualityLevel;

    use super::{frame_rate, quality, speed};

    #[test]
    fn a_pinned_level_is_the_level_alone() {
        assert_eq!(quality(QualityLevel::Level(7), 7), "Q7");
    }

    // The mode and the level are two different things, and the readout says
    // both: under Automatic the level shown is the one the frame rate
    // allowed, which is exactly what someone reading it wants to know.
    #[test]
    fn automatic_is_named_next_to_the_level_it_settled_on() {
        assert_eq!(quality(QualityLevel::Automatic, 14), "Auto\u{b7}Q14");
    }

    #[test]
    fn a_measured_rate_carries_its_frame_time() {
        assert_eq!(frame_rate(60.0), "60 fps\u{b7}16.7 ms");
    }

    // The first second after the dock opens has nothing to report yet, and a
    // "0 fps" there would read as the viewport having frozen.
    #[test]
    fn no_rate_yet_reads_as_measuring_not_as_zero() {
        assert_eq!(frame_rate(0.0), "Measuring\u{2026}");
    }

    #[test]
    fn the_speed_is_whole_studs_per_second() {
        assert_eq!(speed(49.6), "50 studs/s");
    }
}
