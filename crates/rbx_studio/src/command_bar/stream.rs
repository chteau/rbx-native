//! Turning one resume's output into Output panel rows.

use std::ops::Range;

use rbx_lua::LineKind;

use super::Feedback;

/// Splits `kinds` into runs of the same kind, so each prints as one row of
/// its own colour (a warning between two prints must not recolour them).
pub(crate) fn segments(kinds: &[LineKind]) -> Vec<(LineKind, Range<usize>)> {
    let mut out: Vec<(LineKind, Range<usize>)> = Vec::new();
    for (index, kind) in kinds.iter().enumerate() {
        match out.last_mut() {
            Some((last, range)) if last == kind => range.end = index + 1,
            _ => out.push((*kind, index..index + 1)),
        }
    }
    out
}

/// The row for `lines`, all of `kind`.
pub(crate) fn feedback_for(kind: LineKind, lines: &[String]) -> Feedback {
    let text = lines.join("\n");
    match kind {
        LineKind::Print => Feedback::Output(text),
        LineKind::Warn => Feedback::Warning(text),
        LineKind::Error => Feedback::Error(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LineKind::{Error, Print, Warn};

    #[test]
    fn consecutive_lines_of_a_kind_share_a_row() {
        let kinds = [Print, Print, Warn, Print, Error, Error];
        assert_eq!(
            segments(&kinds),
            [(Print, 0..2), (Warn, 2..3), (Print, 3..4), (Error, 4..6)]
        );
    }

    #[test]
    fn no_lines_make_no_rows() {
        assert!(segments(&[]).is_empty());
    }

    #[test]
    fn a_kind_maps_to_its_feedback() {
        let lines = ["a".to_string(), "b".to_string()];
        assert_eq!(feedback_for(Print, &lines), Feedback::Output("a\nb".into()));
        assert_eq!(feedback_for(Warn, &lines), Feedback::Warning("a\nb".into()));
        assert_eq!(feedback_for(Error, &lines), Feedback::Error("a\nb".into()));
    }
}
