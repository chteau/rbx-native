//! Composes the window title the way `rbxstudio`'s viewport corner label
//! composes its own text (`workspace_view::label::status`): the frame rate,
//! once one has been measured, and the flight speed while a fresh reading of
//! it is still worth showing, joined onto the file name.

/// The window title: the file name, then whichever of the frame rate and the
/// flight speed are worth showing right now, joined the way `rbxstudio`'s
/// corner label joins its own fields.
pub(super) fn title(name: &str, fps: Option<f32>, speed: Option<i64>) -> String {
    let mut parts = Vec::new();
    if let Some(fps) = fps {
        parts.push(readout(fps));
    }
    if let Some(speed) = speed {
        parts.push(format!("{speed} studs/s"));
    }

    if parts.is_empty() {
        return name.to_string();
    }
    format!("{name} — {}", parts.join(" \u{b7} "))
}

/// The one-line fps/frame-time format both `rbxview`'s title and `rbxstudio`'s
/// corner label read the same way — `rbx_studio` depends on `rbx_viewer`, never
/// the other way round, so this lives here and `workspace_view::label::status`
/// calls it rather than keeping its own copy of the format string.
pub fn readout(fps: f32) -> String {
    format!("{fps:.0} fps\u{b7}{:.1} ms", 1000.0 / fps)
}

#[cfg(test)]
mod tests {
    use super::title;

    #[test]
    fn no_reading_yet_is_the_plain_file_name() {
        assert_eq!(
            title("rbxview — place.rbxl", None, None),
            "rbxview — place.rbxl"
        );
    }

    #[test]
    fn a_frame_rate_alone_reads_the_way_rbxstudios_corner_label_does() {
        assert_eq!(
            title("rbxview — place.rbxl", Some(60.0), None),
            "rbxview — place.rbxl — 60 fps\u{b7}16.7 ms"
        );
    }

    #[test]
    fn a_fresh_speed_reading_joins_the_frame_rate_rather_than_replacing_it() {
        assert_eq!(
            title("rbxview — place.rbxl", Some(60.0), Some(50)),
            "rbxview — place.rbxl — 60 fps\u{b7}16.7 ms \u{b7} 50 studs/s"
        );
    }

    #[test]
    fn a_speed_reading_with_no_frame_rate_yet_still_reads_cleanly() {
        assert_eq!(
            title("rbxview — place.rbxl", None, Some(50)),
            "rbxview — place.rbxl — 50 studs/s"
        );
    }
}
