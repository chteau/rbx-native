//! What the Play client reports about itself (`Uplink::Device`), as the
//! Device Simulator shows it: a badge line and a first-report Output line.

use rbx_play::protocol::DeviceReport;

/// `client 852×393 · touch · ten-foot off`, for the device badge.
pub(crate) fn summary(report: &DeviceReport) -> String {
    let (w, h) = viewport(report);
    let inputs: Vec<&str> = [
        (report.touch_enabled, "touch"),
        (report.keyboard_enabled, "keyboard"),
        (report.gamepad_enabled, "gamepad"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    let inputs = match inputs.is_empty() {
        true => "no input".to_owned(),
        false => inputs.join(" + "),
    };
    let ten_foot = if report.ten_foot { "on" } else { "off" };
    let mut line = format!("client {w}\u{d7}{h} \u{b7} {inputs} \u{b7} ten-foot {ten_foot}");
    if report.on_screen_keyboard_visible {
        line.push_str(" \u{b7} keyboard shown");
    }
    line
}

/// Whether the client's `ViewportSize` is not the simulated screen. Sober
/// reports no safe-area insets, so the whole window is the viewport.
pub(crate) fn mismatch(report: &DeviceReport, screen: (u32, u32)) -> bool {
    viewport(report) != screen
}

/// The Output line for a Play's first report: every value, by its Roblox
/// name, so a live test leaves a record of what the client said.
pub(crate) fn output_line(report: &DeviceReport, screen: Option<(u32, u32)>) -> String {
    let (w, h) = viewport(report);
    let mut line = format!(
        "Device check: ViewportSize {w}\u{d7}{h}, TouchEnabled {}, KeyboardEnabled {}, \
         GamepadEnabled {}, IsTenFootInterface {}, PreferredInput {}, \
         OnScreenKeyboardVisible {}",
        report.touch_enabled,
        report.keyboard_enabled,
        report.gamepad_enabled,
        report.ten_foot,
        report.preferred_input.as_deref().unwrap_or("unavailable"),
        report.on_screen_keyboard_visible,
    );
    if let Some((sw, sh)) = screen {
        let verdict = match mismatch(report, (sw, sh)) {
            true => "differs",
            false => "matches",
        };
        line.push_str(&format!("; simulated screen {sw}\u{d7}{sh} ({verdict})."));
    } else {
        line.push('.');
    }
    line
}

fn viewport(report: &DeviceReport) -> (u32, u32) {
    let round = |v: f64| v.round().max(0.0) as u32;
    (round(report.viewport_width), round(report.viewport_height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> DeviceReport {
        DeviceReport {
            viewport_width: 852.0,
            viewport_height: 393.0,
            ..DeviceReport::default()
        }
    }

    #[test]
    fn summary_lists_inputs() {
        let touch = DeviceReport {
            touch_enabled: true,
            ..base()
        };
        assert_eq!(
            summary(&touch),
            "client 852\u{d7}393 \u{b7} touch \u{b7} ten-foot off"
        );
        let two = DeviceReport {
            keyboard_enabled: true,
            gamepad_enabled: true,
            ..base()
        };
        assert!(summary(&two).contains("keyboard + gamepad"));
        assert!(summary(&base()).contains("no input"));
    }

    #[test]
    fn summary_flags_keyboard_and_ten_foot() {
        let r = DeviceReport {
            ten_foot: true,
            on_screen_keyboard_visible: true,
            ..base()
        };
        let s = summary(&r);
        assert!(s.contains("ten-foot on"));
        assert!(s.ends_with(" \u{b7} keyboard shown"));
    }

    #[test]
    fn mismatch_compares_the_rounded_viewport() {
        let r = DeviceReport {
            viewport_width: 852.4,
            ..base()
        };
        assert!(!mismatch(&r, (852, 393)));
        assert!(mismatch(&r, (851, 393)));
    }

    #[test]
    fn output_line_verdicts() {
        let r = DeviceReport {
            preferred_input: Some("Touch".to_owned()),
            ..base()
        };
        assert!(output_line(&r, Some((852, 393))).ends_with("(matches)."));
        assert!(output_line(&r, Some((800, 393))).ends_with("(differs)."));
        let none = output_line(&base(), None);
        assert!(none.ends_with("OnScreenKeyboardVisible false."));
        assert!(none.contains("PreferredInput unavailable"));
    }
}
