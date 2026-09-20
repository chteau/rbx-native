// Not `use super::*;`: the parent module's `use gpui_kit::*;` re-exports
// `gpui::test`, which would then shadow `std`'s `#[test]` here and expand
// every plain test below through GPUI's randomized-test machinery instead
// — see `shell::dock`'s own test module for the same guard.
use std::time::UNIX_EPOCH;

use super::{OutputEntry, OutputFilter, OutputLog, RowKind, CAP, SOURCE_MAX_LEN};
use crate::command_bar::Feedback;

fn output(text: &str) -> Feedback {
    Feedback::from_run(Ok(vec![text.to_string()]))
}

fn error(text: &str) -> Feedback {
    Feedback::from_run(Err(text.to_string()))
}

#[test]
fn a_fresh_log_is_empty() {
    let log = OutputLog::default();
    assert!(log.is_empty());
    assert_eq!(log.len(), 0);
}

#[test]
fn pushed_entries_come_back_oldest_first() {
    let mut log = OutputLog::default();
    log.push("print(1)", output("1"));
    log.push("print(2)", output("2"));

    let sources: Vec<&str> = log
        .filtered(OutputFilter::All, "")
        .map(|e| e.source())
        .collect();
    assert_eq!(sources, vec!["print(1)", "print(2)"]);
}

#[test]
fn clear_empties_the_log() {
    let mut log = OutputLog::default();
    log.push("print(1)", output("1"));
    log.clear();
    assert!(log.is_empty());
}

#[test]
fn pushing_past_the_cap_drops_the_oldest_entry() {
    let mut log = OutputLog::default();
    for i in 0..CAP + 5 {
        log.push(&format!("print({i})"), output("x"));
    }

    assert_eq!(log.len(), CAP);
    let first = log.filtered(OutputFilter::All, "").next().unwrap();
    // The five oldest (0..5) should have been dropped to stay at CAP.
    assert_eq!(first.source(), "print(5)");
}

#[test]
fn filter_output_only_hides_errors() {
    let mut log = OutputLog::default();
    log.push("ok", output("done"));
    log.push("bad", error("boom"));

    let sources: Vec<&str> = log
        .filtered(OutputFilter::Output, "")
        .map(|e| e.source())
        .collect();
    assert_eq!(sources, vec!["ok"]);
}

#[test]
fn filter_errors_only_hides_output() {
    let mut log = OutputLog::default();
    log.push("ok", output("done"));
    log.push("bad", error("boom"));

    let sources: Vec<&str> = log
        .filtered(OutputFilter::Errors, "")
        .map(|e| e.source())
        .collect();
    assert_eq!(sources, vec!["bad"]);
}

#[test]
fn filter_all_shows_everything() {
    let mut log = OutputLog::default();
    log.push("ok", output("done"));
    log.push("bad", error("boom"));

    assert_eq!(log.filtered(OutputFilter::All, "").count(), 2);
}

#[test]
fn an_entry_reports_its_own_error_state() {
    let ok_entry = OutputEntry::new("ok", output("done"));
    let err_entry = OutputEntry::new("bad", error("boom"));
    assert!(!ok_entry.is_error());
    assert!(err_entry.is_error());
}

#[test]
fn a_long_source_is_truncated_with_an_ellipsis() {
    let entry = OutputEntry::new(&"x".repeat(SOURCE_MAX_LEN + 20), output("done"));
    let label = entry.truncated_source();
    assert_eq!(label.chars().count(), SOURCE_MAX_LEN + 1);
    assert!(label.ends_with('…'));
}

#[test]
fn filter_default_is_all() {
    assert_eq!(OutputFilter::default(), OutputFilter::All);
}

#[test]
fn a_pushed_warning_is_not_treated_as_an_error() {
    let mut log = OutputLog::default();
    log.push_warning("asset 1: fetching asset 1 failed");

    let entry = log.filtered(OutputFilter::All, "").next().unwrap();
    assert!(!entry.is_error());
}

/// One row, one bucket: a warning is not a run's output and not an error,
/// which is the whole point of giving it a bucket of its own.
#[test]
fn a_warning_shows_under_all_and_warnings_and_nowhere_else() {
    let mut log = OutputLog::default();
    log.push_warning("boom");

    assert_eq!(log.filtered(OutputFilter::All, "").count(), 1);
    assert_eq!(log.filtered(OutputFilter::Warnings, "").count(), 1);
    assert_eq!(log.filtered(OutputFilter::Output, "").count(), 0);
    assert_eq!(log.filtered(OutputFilter::Errors, "").count(), 0);
}

/// The other side of the same rule: the Warnings bucket holds *only*
/// warnings, so a failed run does not leak into it on its way to Errors.
#[test]
fn a_run_never_shows_under_warnings() {
    let mut log = OutputLog::default();
    log.push("print(1)", Feedback::Output("1".into()));
    log.push("boom()", Feedback::Error("boom".into()));

    assert_eq!(log.filtered(OutputFilter::Warnings, "").count(), 0);
    assert_eq!(log.filtered(OutputFilter::All, "").count(), 2);
}

/// The search box narrows *within* the level filter rather than replacing
/// it — the same contract the Errors/Output buckets already have.
#[test]
fn the_search_box_narrows_inside_the_warnings_bucket() {
    let mut log = OutputLog::default();
    log.push_warning("asset 1: not found");
    log.push_warning("asset 2: decode failed");

    assert_eq!(log.filtered(OutputFilter::Warnings, "asset").count(), 2);
    assert_eq!(log.filtered(OutputFilter::Warnings, "decode").count(), 1);
}

#[test]
fn pushing_warnings_past_the_cap_drops_the_oldest() {
    let mut log = OutputLog::default();
    for i in 0..CAP + 5 {
        log.push_warning(&format!("warning {i}"));
    }

    assert_eq!(log.len(), CAP);
}

#[test]
fn an_entry_carries_a_timestamp() {
    let entry = OutputEntry::new("print(1)", output("1"));
    // Not a fixed value to assert against — just confirms `new` actually
    // captured one rather than leaving it at some sentinel.
    assert!(entry.timestamp.duration_since(UNIX_EPOCH).is_ok());
}

#[test]
fn an_entrys_kind_matches_its_feedback() {
    let entry = OutputEntry::new("bad", error("boom"));
    assert_eq!(entry.kind(), RowKind::Error);
}

// --- the search box ---------------------------------------------------

/// Empty means "no narrowing", not "nothing matches" — the box sits on top of
/// the level filter rather than beside it.
#[test]
fn an_empty_query_keeps_every_entry() {
    let mut log = OutputLog::default();
    log.push("print(1)", output("1"));
    log.push("oops()", error("attempt to call a nil value"));

    assert_eq!(log.filtered(OutputFilter::All, "").count(), 2);
}

/// Both halves of the row are searchable, because both are on screen: the
/// command that was run and the result printed beside it.
#[test]
fn a_query_matches_the_source_or_the_result() {
    let mut log = OutputLog::default();
    log.push("print(1)", output("hello"));
    log.push("workspace:FindFirstChild('Baseplate')", output("nil"));

    let by_source: Vec<&str> = log
        .filtered(OutputFilter::All, "findfirstchild")
        .map(|e| e.source())
        .collect();
    assert_eq!(by_source, vec!["workspace:FindFirstChild('Baseplate')"]);

    let by_result: Vec<&str> = log
        .filtered(OutputFilter::All, "hello")
        .map(|e| e.source())
        .collect();
    assert_eq!(by_result, vec!["print(1)"]);
}

/// Nobody searching a log for an error types it the way the error did.
#[test]
fn a_query_ignores_case() {
    let mut log = OutputLog::default();
    log.push("Workspace", output("Instance"));

    assert_eq!(log.filtered(OutputFilter::All, "workspace").count(), 1);
    assert_eq!(log.filtered(OutputFilter::All, "INSTANCE").count(), 1);
}

/// The two narrow together rather than either one winning: an error that
/// matches the text is still hidden under the Output level, and an output row
/// that does not match the text is still hidden under a query.
#[test]
fn the_search_narrows_within_the_level_filter_rather_than_replacing_it() {
    let mut log = OutputLog::default();
    log.push("boom()", error("boom"));
    log.push("print('boom')", output("boom"));

    assert_eq!(log.filtered(OutputFilter::All, "boom").count(), 2);
    assert_eq!(log.filtered(OutputFilter::Errors, "boom").count(), 1);
    assert_eq!(log.filtered(OutputFilter::Output, "boom").count(), 1);
    assert_eq!(log.filtered(OutputFilter::Output, "nothing").count(), 0);
}
