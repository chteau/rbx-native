// Not `use super::*;`: the parent module's `use gpui_kit::*;` re-exports
// `gpui::test`, which would then shadow `std`'s `#[test]` here and expand
// every plain test below through GPUI's randomized-test machinery instead
// — see `shell::dock`'s own test module for the same guard.
use std::time::UNIX_EPOCH;

use super::{
    game, ContextFilter, OutputEntry, OutputFilter, OutputLog, RowKind, CAP, SOURCE_MAX_LEN,
};
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
        .filtered(OutputFilter::All, ContextFilter::All, "")
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
    let first = log
        .filtered(OutputFilter::All, ContextFilter::All, "")
        .next()
        .unwrap();
    // The five oldest (0..5) should have been dropped to stay at CAP.
    assert_eq!(first.source(), "print(5)");
}

#[test]
fn filter_output_only_hides_errors() {
    let mut log = OutputLog::default();
    log.push("ok", output("done"));
    log.push("bad", error("boom"));

    let sources: Vec<&str> = log
        .filtered(OutputFilter::Output, ContextFilter::All, "")
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
        .filtered(OutputFilter::Errors, ContextFilter::All, "")
        .map(|e| e.source())
        .collect();
    assert_eq!(sources, vec!["bad"]);
}

#[test]
fn filter_all_shows_everything() {
    let mut log = OutputLog::default();
    log.push("ok", output("done"));
    log.push("bad", error("boom"));

    assert_eq!(
        log.filtered(OutputFilter::All, ContextFilter::All, "")
            .count(),
        2
    );
}

#[test]
fn an_entry_reports_its_own_error_state() {
    let ok_entry = OutputEntry::new(0, "ok", output("done"));
    let err_entry = OutputEntry::new(0, "bad", error("boom"));
    assert!(!ok_entry.is_error());
    assert!(err_entry.is_error());
}

#[test]
fn a_long_source_is_truncated_with_an_ellipsis() {
    let entry = OutputEntry::new(0, &"x".repeat(SOURCE_MAX_LEN + 20), output("done"));
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

    let entry = log
        .filtered(OutputFilter::All, ContextFilter::All, "")
        .next()
        .unwrap();
    assert!(!entry.is_error());
}

/// One row, one bucket: a warning is not a run's output and not an error,
/// which is the whole point of giving it a bucket of its own.
#[test]
fn a_warning_shows_under_all_and_warnings_and_nowhere_else() {
    let mut log = OutputLog::default();
    log.push_warning("boom");

    assert_eq!(
        log.filtered(OutputFilter::All, ContextFilter::All, "")
            .count(),
        1
    );
    assert_eq!(
        log.filtered(OutputFilter::Warnings, ContextFilter::All, "")
            .count(),
        1
    );
    assert_eq!(
        log.filtered(OutputFilter::Output, ContextFilter::All, "")
            .count(),
        0
    );
    assert_eq!(
        log.filtered(OutputFilter::Errors, ContextFilter::All, "")
            .count(),
        0
    );
}

/// The other side of the same rule: the Warnings bucket holds *only*
/// warnings, so a failed run does not leak into it on its way to Errors.
#[test]
fn a_run_never_shows_under_warnings() {
    let mut log = OutputLog::default();
    log.push("print(1)", Feedback::Output("1".into()));
    log.push("boom()", Feedback::Error("boom".into()));

    assert_eq!(
        log.filtered(OutputFilter::Warnings, ContextFilter::All, "")
            .count(),
        0
    );
    assert_eq!(
        log.filtered(OutputFilter::All, ContextFilter::All, "")
            .count(),
        2
    );
}

/// The search box narrows *within* the level filter rather than replacing
/// it — the same contract the Errors/Output buckets already have.
#[test]
fn the_search_box_narrows_inside_the_warnings_bucket() {
    let mut log = OutputLog::default();
    log.push_warning("asset 1: not found");
    log.push_warning("asset 2: decode failed");

    assert_eq!(
        log.filtered(OutputFilter::Warnings, ContextFilter::All, "asset")
            .count(),
        2
    );
    assert_eq!(
        log.filtered(OutputFilter::Warnings, ContextFilter::All, "decode")
            .count(),
        1
    );
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
    let entry = OutputEntry::new(0, "print(1)", output("1"));
    // Not a fixed value to assert against — just confirms `new` actually
    // captured one rather than leaving it at some sentinel.
    assert!(entry.timestamp.duration_since(UNIX_EPOCH).is_ok());
}

#[test]
fn an_entrys_kind_matches_its_feedback() {
    let entry = OutputEntry::new(0, "bad", error("boom"));
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

    assert_eq!(
        log.filtered(OutputFilter::All, ContextFilter::All, "")
            .count(),
        2
    );
}

/// Both halves of the row are searchable, because both are on screen: the
/// command that was run and the result printed beside it.
#[test]
fn a_query_matches_the_source_or_the_result() {
    let mut log = OutputLog::default();
    log.push("print(1)", output("hello"));
    log.push("workspace:FindFirstChild('Baseplate')", output("nil"));

    let by_source: Vec<&str> = log
        .filtered(OutputFilter::All, ContextFilter::All, "findfirstchild")
        .map(|e| e.source())
        .collect();
    assert_eq!(by_source, vec!["workspace:FindFirstChild('Baseplate')"]);

    let by_result: Vec<&str> = log
        .filtered(OutputFilter::All, ContextFilter::All, "hello")
        .map(|e| e.source())
        .collect();
    assert_eq!(by_result, vec!["print(1)"]);
}

/// Nobody searching a log for an error types it the way the error did.
#[test]
fn a_query_ignores_case() {
    let mut log = OutputLog::default();
    log.push("Workspace", output("Instance"));

    assert_eq!(
        log.filtered(OutputFilter::All, ContextFilter::All, "workspace")
            .count(),
        1
    );
    assert_eq!(
        log.filtered(OutputFilter::All, ContextFilter::All, "INSTANCE")
            .count(),
        1
    );
}

/// The two narrow together rather than either one winning: an error that
/// matches the text is still hidden under the Output level, and an output row
/// that does not match the text is still hidden under a query.
#[test]
fn the_search_narrows_within_the_level_filter_rather_than_replacing_it() {
    let mut log = OutputLog::default();
    log.push("boom()", error("boom"));
    log.push("print('boom')", output("boom"));

    assert_eq!(
        log.filtered(OutputFilter::All, ContextFilter::All, "boom")
            .count(),
        2
    );
    assert_eq!(
        log.filtered(OutputFilter::Errors, ContextFilter::All, "boom")
            .count(),
        1
    );
    assert_eq!(
        log.filtered(OutputFilter::Output, ContextFilter::All, "boom")
            .count(),
        1
    );
    assert_eq!(
        log.filtered(OutputFilter::Output, ContextFilter::All, "nothing")
            .count(),
        0
    );
}

#[test]
fn the_same_entry_pushed_twice_at_once_is_one_row() {
    let mut log = OutputLog::default();
    log.push_once("Rig", error("UserId must be a number"));
    log.push_once("Rig", error("UserId must be a number"));
    assert_eq!(log.len(), 1);
    log.push_once("Rig", error("Another message"));
    log.push_once("Other", error("Another message"));
    assert_eq!(log.len(), 3);
}

#[test]
fn a_game_error_keeps_its_side_location_and_stack() {
    use super::game::Context;
    let mut log = OutputLog::default();
    log.push_game(
        Context::Client,
        error("Players.T.PlayerScripts.Hud:4: boom\n    Stack Begin\n    Script 'Players.T.PlayerScripts.Hud', Line 4\n    Stack End"),
    );
    let entry = log
        .filtered(OutputFilter::Errors, ContextFilter::All, "")
        .next()
        .expect("an error row");
    assert_eq!(entry.source(), "Client");
    let game = entry.game.as_ref().expect("a game row");
    assert_eq!(game.context, Context::Client);
    assert_eq!(game.text, "Players.T.PlayerScripts.Hud:4: boom");
    assert_eq!(
        game.location.as_ref().map(|l| l.short()),
        Some("Hud:4".into())
    );
    assert_eq!(game.frames.len(), 1);
    // The search box still finds it by the script's name.
    assert_eq!(
        log.filtered(OutputFilter::All, ContextFilter::All, "hud")
            .count(),
        1
    );
}

#[test]
fn an_info_message_has_its_own_bucket() {
    let mut log = OutputLog::default();
    log.push_game(
        game::Context::Server,
        Feedback::Info("TestService: ok".into()),
    );
    log.push_game(game::Context::Server, output("hello"));
    let info: Vec<_> = log
        .filtered(OutputFilter::Info, ContextFilter::All, "")
        .collect();
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].kind(), RowKind::Info);
    assert_eq!(
        log.filtered(OutputFilter::Output, ContextFilter::All, "")
            .count(),
        1
    );
}

#[test]
fn the_context_filter_splits_edit_server_and_client() {
    let mut log = OutputLog::default();
    log.push("print(1)", output("1"));
    log.push_warning("texture fell back");
    log.push_game(game::Context::Server, output("from server"));
    log.push_game(game::Context::Client, output("from client"));
    let count = |context| log.filtered(OutputFilter::All, context, "").count();
    assert_eq!(count(ContextFilter::All), 4);
    assert_eq!(count(ContextFilter::Edit), 2);
    assert_eq!(count(ContextFilter::Server), 1);
    assert_eq!(count(ContextFilter::Client), 1);
    let client = log
        .filtered(OutputFilter::All, ContextFilter::Client, "")
        .next()
        .unwrap();
    assert_eq!(client.context(), Some(game::Context::Client));
}

fn table_print() -> Vec<Vec<rbx_lua::PrintPart>> {
    vec![vec![rbx_lua::PrintPart::Table(rbx_lua::TableDump {
        address: "0x1".into(),
        entries: Vec::new(),
        more: 0,
    })]]
}

#[test]
fn a_run_keeps_its_print_structure_only_when_it_printed_a_table() {
    let mut log = OutputLog::default();
    log.push_run(
        "print(1)",
        output("1"),
        vec![vec![rbx_lua::PrintPart::Text("1".into())]],
    );
    log.push_run("print({})", output("table: 0x1"), table_print());
    let entries: Vec<_> = log
        .filtered(OutputFilter::All, ContextFilter::All, "")
        .collect();
    assert!(entries[0].printed.is_empty());
    assert_eq!(entries[1].printed, table_print());
}

#[test]
fn a_clicked_table_flips_against_the_default() {
    let mut log = OutputLog::default();
    log.push_run("print({})", output("table: 0x1"), table_print());
    let id = log.entries[0].id;
    assert!(!log.table_open(id, &[0, 0], false));
    assert!(log.table_open(id, &[0, 0], true));
    log.toggle_table(id, vec![0, 0]);
    assert!(log.table_open(id, &[0, 0], false));
    assert!(!log.table_open(id, &[0, 0], true));
    // A sibling path is untouched.
    assert!(!log.table_open(id, &[0, 1], false));
    log.toggle_table(id, vec![0, 0]);
    assert!(!log.table_open(id, &[0, 0], false));
}

#[test]
fn table_state_goes_with_its_entry() {
    let mut log = OutputLog::default();
    log.push_run("print({})", output("table: 0x1"), table_print());
    let first = log.entries[0].id;
    log.toggle_table(first, vec![0, 0]);
    for i in 0..CAP {
        log.push(&format!("print({i})"), output("x"));
    }
    assert!(log.flipped.is_empty(), "dropped entry's state is gone");
    let id = log.entries[0].id;
    assert_ne!(id, first, "ids are never reused");
    log.toggle_table(id, vec![0]);
    log.clear();
    assert!(log.flipped.is_empty());
}
