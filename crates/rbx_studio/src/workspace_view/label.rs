//! The corner label: what the viewport is drawing at, how fast it is flying,
//! and — while the Stats toggle is on (see `Shell::set_stats_shown`, the
//! Viewport panel overflow menu item next to Orthographic) — how fast it is
//! actually drawing. Real Studio's own equivalent is `Window > Performance >
//! Stats`; this editor has no `Window` menu yet, so the toggle lives next to
//! the other per-viewport debug affordance already there.

use gpui_kit::SharedString;
use rbx_viewer::QualityLevel;

/// The quality level, always; the frame rate while Stats is on; the flight
/// speed while it is worth showing.
///
/// `Automatic` says so rather than showing the bare level: a level that moves on
/// its own is otherwise read as the renderer misbehaving.
pub(super) fn status(
    mode: QualityLevel,
    level: u8,
    fps: Option<f32>,
    speed: Option<f32>,
) -> SharedString {
    let quality = match mode {
        QualityLevel::Automatic => format!("Auto\u{b7}Q{level}"),
        QualityLevel::Level(_) => format!("Q{level}"),
    };

    let mut parts = vec![quality];
    if let Some(fps) = fps {
        // Frame time is exactly 1/fps over the same window `Stats` measured
        // it in, not a separate approximation — see `stats::fps`. Formatted
        // by `rbx_viewer::fps_readout`, which `rbxview`'s own title bar reads
        // the same way.
        parts.push(rbx_viewer::fps_readout(fps));
    }
    if let Some(speed) = speed {
        parts.push(format!("{} studs/s", speed.round() as i64));
    }

    SharedString::from(parts.join(" \u{b7} "))
}

#[cfg(test)]
mod tests {
    use rbx_viewer::QualityLevel;

    use super::status;

    #[test]
    fn a_pinned_level_is_the_level_alone() {
        assert_eq!(status(QualityLevel::Level(7), 7, None, None), "Q7");
    }

    // The mode and the level are two different things, and the label says both:
    // under Automatic the level shown is the one the frame rate allowed, which
    // is exactly what a user reading the label wants to know.
    #[test]
    fn automatic_is_named_next_to_the_level_it_settled_on() {
        assert_eq!(
            status(QualityLevel::Automatic, 14, None, None),
            "Auto\u{b7}Q14"
        );
    }

    #[test]
    fn a_fresh_speed_reading_joins_the_level() {
        assert_eq!(
            status(QualityLevel::Automatic, 21, None, Some(49.6)),
            "Auto\u{b7}Q21 \u{b7} 50 studs/s"
        );
    }

    // The Stats toggle: `None` (off) leaves the label exactly as it read
    // before this readout existed; `Some` (on) inserts fps and its exact
    // frame time (1000 / fps) between the quality and the speed.
    #[test]
    fn stats_off_reads_the_same_as_before_the_readout_existed() {
        assert_eq!(status(QualityLevel::Level(7), 7, None, None), "Q7");
    }

    #[test]
    fn stats_on_inserts_fps_and_frame_time_before_the_speed() {
        assert_eq!(
            status(QualityLevel::Level(7), 7, Some(60.0), Some(49.6)),
            "Q7 \u{b7} 60 fps\u{b7}16.7 ms \u{b7} 50 studs/s"
        );
    }

    #[test]
    fn stats_on_with_no_speed_to_show_still_reads_cleanly() {
        assert_eq!(
            status(QualityLevel::Automatic, 14, Some(29.5), None),
            "Auto\u{b7}Q14 \u{b7} 30 fps\u{b7}33.9 ms"
        );
    }
}
