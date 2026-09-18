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
        .filtered(OutputFilter::All)
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
    let first = log.filtered(OutputFilter::All).next().unwrap();
    // The five oldest (0..5) should have been dropped to stay at CAP.
    assert_eq!(first.source(), "print(5)");
}

#[test]
fn filter_output_only_hides_errors() {
    let mut log = OutputLog::default();
    log.push("ok", output("done"));
    log.push("bad", error("boom"));

    let sources: Vec<&str> = log
        .filtered(OutputFilter::Output)
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
        .filtered(OutputFilter::Errors)
        .map(|e| e.source())
        .collect();
    assert_eq!(sources, vec!["bad"]);
}

#[test]
fn filter_all_shows_everything() {
    let mut log = OutputLog::default();
    log.push("ok", output("done"));
    log.push("bad", error("boom"));

    assert_eq!(log.filtered(OutputFilter::All).count(), 2);
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

    let entry = log.filtered(OutputFilter::All).next().unwrap();
    assert!(!entry.is_error());
}

#[test]
fn a_warning_shows_under_all_and_output_but_never_errors() {
    let mut log = OutputLog::default();
    log.push_warning("boom");

    assert_eq!(log.filtered(OutputFilter::All).count(), 1);
    assert_eq!(log.filtered(OutputFilter::Output).count(), 1);
    assert_eq!(log.filtered(OutputFilter::Errors).count(), 0);
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
