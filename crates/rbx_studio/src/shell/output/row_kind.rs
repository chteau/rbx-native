//! Per-row presentation for the Output dock, split out of `output.rs` per
//! `GUIDELINES.md` §6 once the per-kind styling and timestamp work pushed
//! that file past its line budget: which color/icon family a [`Feedback`]
//! maps to (see [`RowKind`]), and the `HH:MM:SS.SSS` format `output`'s
//! `OutputEntry::timestamp_label` paints when "Show Timestamp" is on.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName;
use gpui_kit::SharedString;

use crate::command_bar::Feedback;

/// `HH:MM:SS.SSS`.
///
/// UTC, not the machine's local offset: the `time` crate (already in this
/// workspace's dependency tree transitively) gates its local-offset lookup
/// behind an `unsound_local_offset` feature flag — it's unsound to call from
/// a multi-threaded process, which this editor is — and there's no other
/// timezone source here to reach for instead.
// ponytail: UTC-only clock; swap in a vetted local-time source (or accept
// the `time` crate's unsoundness caveat explicitly) if local display ever
// matters more than sidestepping that.
pub(super) fn format_timestamp(time: SystemTime) -> SharedString {
    let elapsed = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_secs = elapsed.as_secs();
    let hours = (total_secs / 3600) % 24;
    let minutes = (total_secs / 60) % 60;
    let seconds = total_secs % 60;
    let millis = elapsed.subsec_millis();
    SharedString::from(format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}"))
}

/// Which color/icon family a row's [`Feedback`] maps to — Studio's Output
/// window docs give each of its four message kinds its own color and icon;
/// `TestService.Message`'s blue/info kind has no producer anywhere in this
/// codebase yet (there is no sandbox to run a `TestService` script in), so
/// it has no variant here either — see `ROADMAP.md`'s Output window bullet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowKind {
    /// `print` and a script's own successful run — Studio's docs call this
    /// the "default/black text color", i.e. no override at all rather than
    /// a distinct one; see [`RowKind::color`].
    Success,
    Warning,
    Error,
}

/// Which theme token a [`RowKind`] paints with, kept as its own enum rather
/// than resolving straight to an `Hsla` here so the mapping is testable
/// without a live `Theme` (see this module's tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowColor {
    /// No override — the row keeps whatever text color it already had.
    Default,
    Warning,
    Danger,
}

impl RowKind {
    pub(super) fn of(feedback: &Feedback) -> Self {
        match feedback {
            Feedback::Error(_) => RowKind::Error,
            Feedback::Warning(_) => RowKind::Warning,
            // `Idle` never reaches an `OutputEntry`: `run_command` always logs
            // a `Feedback::from_run` result (`Output`/`Error`), and
            // `push_warning` always logs `Warning` — there's no path that
            // logs `Idle`. Folded into `Success` rather than given its own
            // unreachable case.
            Feedback::Idle | Feedback::Output(_) => RowKind::Success,
        }
    }

    pub(super) fn icon(self) -> IconName {
        match self {
            RowKind::Success => IconName::CircleCheckBig,
            RowKind::Warning => IconName::CircleAlert,
            RowKind::Error => IconName::CircleX,
        }
    }

    pub(super) fn color(self) -> RowColor {
        match self {
            RowKind::Success => RowColor::Default,
            RowKind::Warning => RowColor::Warning,
            RowKind::Error => RowColor::Danger,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui_kit::assets::IconName;

    use super::{format_timestamp, RowColor, RowKind, UNIX_EPOCH};
    use crate::command_bar::Feedback;

    fn output(text: &str) -> Feedback {
        Feedback::from_run(Ok(vec![text.to_string()]))
    }

    fn error(text: &str) -> Feedback {
        Feedback::from_run(Err(text.to_string()))
    }

    fn warning(text: &str) -> Feedback {
        Feedback::Warning(text.to_string())
    }

    #[test]
    fn timestamps_format_as_hh_mm_ss_millis() {
        let time = UNIX_EPOCH + Duration::new(3661, 500_000_000);
        assert_eq!(format_timestamp(time), "01:01:01.500");
    }

    #[test]
    fn a_timestamp_before_ten_hours_is_zero_padded() {
        let time = UNIX_EPOCH + Duration::new(5, 9_000_000);
        assert_eq!(format_timestamp(time), "00:00:05.009");
    }

    #[test]
    fn a_timestamp_wraps_at_24_hours() {
        // 90000s = 25h, which Studio's own clock shows as 01:00:00, not 25:00:00.
        let time = UNIX_EPOCH + Duration::new(90_000, 0);
        assert_eq!(format_timestamp(time), "01:00:00.000");
    }

    #[test]
    fn success_output_maps_to_the_default_color_and_a_check_icon() {
        let kind = RowKind::of(&output("done"));
        assert_eq!(kind, RowKind::Success);
        assert_eq!(kind.color(), RowColor::Default);
        assert_eq!(kind.icon(), IconName::CircleCheckBig);
    }

    #[test]
    fn warning_maps_to_the_warning_color_and_an_alert_icon() {
        let kind = RowKind::of(&warning("careful"));
        assert_eq!(kind, RowKind::Warning);
        assert_eq!(kind.color(), RowColor::Warning);
        assert_eq!(kind.icon(), IconName::CircleAlert);
    }

    #[test]
    fn error_maps_to_the_danger_color_and_an_x_icon() {
        let kind = RowKind::of(&error("boom"));
        assert_eq!(kind, RowKind::Error);
        assert_eq!(kind.color(), RowColor::Danger);
        assert_eq!(kind.icon(), IconName::CircleX);
    }
}
