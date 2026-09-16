use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{is, is_script, label, read, write, SOURCE_PROPERTY};
use crate::history::{History, DEFAULT_CAP};

/// A place holding one instance of `class`, with `Source` already set when
/// `source` says so — a script read out of a real file always has one, a
/// script inserted through the Explorer does not yet.
fn place(class: &str, source: Option<&str>) -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let script = dom.new_instance(class, "Greeter", Some(workspace));
    if let Some(source) = source {
        let _ = dom.set_property(script, SOURCE_PROPERTY, Variant::String(source.to_owned()));
    }
    (dom, script)
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn all_three_script_classes_are_editable() {
    for class in ["Script", "LocalScript", "ModuleScript"] {
        let (dom, script) = place(class, Some(""));
        assert!(
            is_script(&dom, &database(), script),
            "{class} should open in the script editor"
        );
    }
}

#[test]
fn an_ordinary_instance_is_not_editable() {
    for class in ["Part", "Folder", "Workspace", "StringValue"] {
        let (dom, instance) = place(class, None);
        assert!(
            !is_script(&dom, &database(), instance),
            "{class} has no Source and must not open an editor tab"
        );
    }
}

#[test]
fn a_referent_the_dom_no_longer_holds_is_not_editable() {
    let (dom, _) = place("Script", Some(""));
    assert!(!is_script(&dom, &database(), Ref::new(9999)));
}

#[test]
fn reading_gives_back_exactly_what_was_stored() {
    let (dom, script) = place("Script", Some("print('hi')\n"));
    assert_eq!(read(&dom, script).as_deref(), Some("print('hi')\n"));
}

#[test]
fn a_script_without_a_source_property_reads_as_none() {
    let (dom, script) = place("Script", None);
    assert!(is_script(&dom, &database(), script));
    assert_eq!(read(&dom, script), None);
}

#[test]
fn writing_then_reading_round_trips_the_source() {
    let (mut dom, script) = place("Script", Some("old"));
    assert!(write(&mut dom, script, "new"));
    assert_eq!(read(&dom, script).as_deref(), Some("new"));
}

#[test]
fn writing_creates_the_property_when_the_script_had_none() {
    let (mut dom, script) = place("Script", None);
    assert!(write(&mut dom, script, "print(1)"));
    assert_eq!(read(&dom, script).as_deref(), Some("print(1)"));
}

#[test]
fn leading_and_trailing_whitespace_survives_the_round_trip() {
    // The Properties panel's own commit path trims what it writes, which is
    // why this module writes the DOM directly: a trailing newline and a
    // leading blank line are part of the code, not padding around it.
    let source = "\n\nlocal x = 1\n\t\n";
    let (mut dom, script) = place("Script", Some(""));
    assert!(write(&mut dom, script, source));
    assert_eq!(read(&dom, script).as_deref(), Some(source));
}

#[test]
fn writing_the_value_already_there_reports_no_change() {
    // The guard that keeps re-seeding an editor from the DOM after an undo
    // from immediately pushing that same text back as a new edit.
    let (mut dom, script) = place("Script", Some("unchanged"));
    assert!(
        !write(&mut dom, script, "unchanged"),
        "an identical write must not count as a change"
    );
    assert_eq!(read(&dom, script).as_deref(), Some("unchanged"));
}

#[test]
fn writing_to_a_referent_the_dom_no_longer_holds_reports_no_change() {
    let (mut dom, _) = place("Script", Some(""));
    assert!(!write(&mut dom, Ref::new(9999), "anything"));
}

#[test]
fn the_label_is_the_instance_name_and_follows_a_rename() {
    let (mut dom, script) = place("Script", Some(""));
    assert_eq!(label(&dom, script).as_deref(), Some("Greeter"));

    let _ = dom.set_name(script, "Renamed");
    assert_eq!(
        label(&dom, script).as_deref(),
        Some("Renamed"),
        "the tab label is read from the DOM, never cached at open time"
    );
}

/// What `shell::scripts::commit_script` does — snapshot, write, then attach
/// the change log that write produced — using the same pieces it calls, so
/// the undo path can be exercised without a window around it. The log is what
/// lets an undone `Source` take the fast viewport patch instead of a full
/// reload; `shell::history`'s own tests read it back through the classifier.
fn commit(history: &mut History, dom: &mut WeakDom, script: Ref, text: &str) {
    dom.take_changes();
    history.push(dom.clone());
    write(dom, script, text);
    history.record_changes(dom.take_changes());
}

#[test]
fn an_edit_to_a_scripts_source_undoes_and_redoes_cleanly() {
    let (mut dom, script) = place("Script", Some("print(1)\n"));
    let mut history = History::new(DEFAULT_CAP);

    commit(&mut history, &mut dom, script, "print(2)\n");
    assert_eq!(read(&dom, script).as_deref(), Some("print(2)\n"));

    let (undone, _) = history.undo(dom.clone()).expect("the edit to undo");
    assert_eq!(
        read(&undone, script).as_deref(),
        Some("print(1)\n"),
        "Ctrl+Z must put the script's source back"
    );

    let (redone, _) = history.redo(undone).expect("the edit to redo");
    assert_eq!(read(&redone, script).as_deref(), Some("print(2)\n"));
}

#[test]
fn each_typing_burst_is_its_own_undo_step() {
    // The debounce means one write per pause, not per keystroke, so stepping
    // back twice must land on the two texts that were actually committed.
    let (mut dom, script) = place("Script", Some("one"));
    let mut history = History::new(DEFAULT_CAP);

    commit(&mut history, &mut dom, script, "two");
    commit(&mut history, &mut dom, script, "three");

    let (once, _) = history.undo(dom.clone()).expect("the second edit to undo");
    assert_eq!(read(&once, script).as_deref(), Some("two"));

    let (twice, _) = history.undo(once).expect("the first edit to undo");
    assert_eq!(read(&twice, script).as_deref(), Some("one"));
}

#[test]
fn an_undone_source_no_longer_matches_what_the_editor_last_synced() {
    // The signal `Shell::resync_scripts` re-seeds an open tab on: the DOM
    // stopped agreeing with the text that tab last had in common with it.
    let (mut dom, script) = place("Script", Some("before"));
    let mut history = History::new(DEFAULT_CAP);

    commit(&mut history, &mut dom, script, "after");
    let synced = "after";
    assert!(is(&dom, script, synced), "nothing to re-seed yet");

    let (undone, _) = history.undo(dom).expect("the edit to undo");
    assert!(
        !is(&undone, script, synced),
        "the undo must be visible to the editor as a mismatch"
    );
}
