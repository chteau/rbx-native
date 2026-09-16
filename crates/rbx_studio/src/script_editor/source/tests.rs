use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{is_script, label, read, write, SOURCE_PROPERTY};

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
