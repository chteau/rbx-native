//! The corner label: what the viewport is drawing at, and how fast it is flying.

use gpui_kit::SharedString;
use rbx_viewer::QualityLevel;

/// The quality level, always, and the flight speed while it is worth showing.
///
/// `Automatic` says so rather than showing the bare level: a level that moves on
/// its own is otherwise read as the renderer misbehaving.
pub(super) fn status(mode: QualityLevel, level: u8, speed: Option<f32>) -> SharedString {
    let quality = match mode {
        QualityLevel::Automatic => format!("Auto\u{b7}Q{level}"),
        QualityLevel::Level(_) => format!("Q{level}"),
    };
    let Some(speed) = speed else {
        return SharedString::from(quality);
    };

    SharedString::from(format!("{quality} \u{b7} {} studs/s", speed.round() as i64))
}

#[cfg(test)]
mod tests {
    use rbx_viewer::QualityLevel;

    use super::status;

    #[test]
    fn a_pinned_level_is_the_level_alone() {
        assert_eq!(status(QualityLevel::Level(7), 7, None), "Q7");
    }

    // The mode and the level are two different things, and the label says both:
    // under Automatic the level shown is the one the frame rate allowed, which
    // is exactly what a user reading the label wants to know.
    #[test]
    fn automatic_is_named_next_to_the_level_it_settled_on() {
        assert_eq!(status(QualityLevel::Automatic, 14, None), "Auto\u{b7}Q14");
    }

    #[test]
    fn a_fresh_speed_reading_joins_the_level() {
        assert_eq!(
            status(QualityLevel::Automatic, 21, Some(49.6)),
            "Auto\u{b7}Q21 \u{b7} 50 studs/s"
        );
    }
}
