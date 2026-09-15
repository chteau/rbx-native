//! What the label above the input shows: the outcome of the last run, and
//! nothing before that — Studio's own bar keeps no scrollback either.

use gpui_kit::SharedString;

/// Past this many characters the label is cut with an ellipsis: long enough to
/// read a one-line result, short enough that a `print` loop or a long stack
/// trace cannot stretch the bar itself.
const MAX_LEN: usize = 200;

/// The command bar's own state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Feedback {
    #[default]
    Idle,
    Output(String),
    Error(String),
    /// A warning pushed by something other than a Command Bar run — see
    /// `shell::output::OutputLog::push_warning`. Never produced by
    /// [`Feedback::from_run`]: nothing about a script's own result is a
    /// warning today, only what the Output dock's other callers push.
    Warning(String),
}

impl Feedback {
    /// Turns one run's outcome into feedback. A successful run with nothing
    /// printed is kept distinct from [`Feedback::Idle`] — it did run, it just
    /// had nothing to say — which is why the label still changes.
    pub(crate) fn from_run(result: Result<Vec<String>, String>) -> Self {
        match result {
            Ok(lines) => Feedback::Output(lines.join("\n")),
            Err(message) => Feedback::Error(message),
        }
    }

    pub(crate) fn is_error(&self) -> bool {
        matches!(self, Feedback::Error(_))
    }

    /// The text to paint: blank before the first run, `(no output)` for a
    /// silent success, otherwise the captured text truncated to one line.
    pub(crate) fn label(&self) -> SharedString {
        match self {
            Feedback::Idle => SharedString::default(),
            Feedback::Output(text) if text.is_empty() => SharedString::from("(no output)"),
            Feedback::Output(text) => truncate(text),
            Feedback::Error(text) => truncate(&format!("Error: {text}")),
            Feedback::Warning(text) => truncate(&format!("Warning: {text}")),
        }
    }
}

/// Collapses embedded newlines (a script can `print` several lines) into one
/// row, then cuts it to [`MAX_LEN`] characters.
fn truncate(text: &str) -> SharedString {
    let flat = text.replace('\n', "  ");
    match flat.char_indices().nth(MAX_LEN) {
        Some((cut, _)) => SharedString::from(format!("{}…", &flat[..cut])),
        None => SharedString::from(flat),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_shows_nothing() {
        assert_eq!(Feedback::Idle.label(), SharedString::default());
    }

    #[test]
    fn a_silent_success_says_so_rather_than_showing_a_blank_label() {
        let feedback = Feedback::from_run(Ok(Vec::new()));
        assert!(!feedback.is_error());
        assert_eq!(feedback.label(), SharedString::from("(no output)"));
    }

    #[test]
    fn printed_lines_are_joined_and_shown_as_output() {
        let feedback = Feedback::from_run(Ok(vec!["a".to_string(), "b".to_string()]));
        assert!(!feedback.is_error());
        assert_eq!(feedback.label(), SharedString::from("a  b"));
    }

    #[test]
    fn an_error_is_flagged_and_prefixed() {
        let feedback = Feedback::from_run(Err("boom".to_string()));
        assert!(feedback.is_error());
        assert_eq!(feedback.label(), SharedString::from("Error: boom"));
    }

    #[test]
    fn a_long_result_is_truncated_with_an_ellipsis() {
        let feedback = Feedback::from_run(Ok(vec!["x".repeat(MAX_LEN + 50)]));
        let label = feedback.label();
        assert_eq!(label.chars().count(), MAX_LEN + 1);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn a_warning_is_not_flagged_as_an_error() {
        let feedback = Feedback::Warning("texture fell back to default".to_string());
        assert!(!feedback.is_error());
    }

    #[test]
    fn a_warning_is_labeled_and_prefixed() {
        let feedback = Feedback::Warning("boom".to_string());
        assert_eq!(feedback.label(), SharedString::from("Warning: boom"));
    }
}
