//! The decidable halves of the Explorer's row affordances: which context
//! menu rows are live for a given row and selection, and which rows can be
//! renamed at all. Everything else here needs a live window (see
//! `shell::group`'s own tests for why this codebase's `Shell` methods stop
//! being unit-testable past their `push_history`/`take_changes` pair).

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::menu::availability;
use super::rename::renameable;

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

/// A `Workspace` with a `Model` holding one `Part`, plus a loose `Part`
/// beside the model — enough for every row of the menu to have both answers
/// somewhere in it.
fn place() -> (WeakDom, Ref, Ref, Ref, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Model", Some(workspace));
    let inside = dom.new_instance("Part", "Part", Some(model));
    let loose = dom.new_instance("Part", "Part", Some(workspace));
    (dom, workspace, model, inside, loose)
}

#[test]
fn an_ordinary_row_offers_every_clipboard_action() {
    let (dom, _, _, _, loose) = place();
    let live = availability(&dom, &database(), &[loose], true, loose);
    assert!(live.clipboard);
    assert!(live.rename);
    assert!(live.delete);
    // A lone part has a parent to be wrapped into, but is no `Model`.
    assert!(live.group);
    assert!(!live.ungroup);
}

#[test]
fn a_service_row_greys_what_it_cannot_do() {
    let (dom, workspace, _, _, _) = place();
    let live = availability(&dom, &database(), &[workspace], true, workspace);
    // `clipboard::copyable` drops a service, and so does `group`'s own
    // common-parent rule.
    assert!(!live.clipboard);
    assert!(!live.group);
    assert!(!live.rename);
}

#[test]
fn a_model_row_offers_ungroup() {
    let (dom, _, model, _, _) = place();
    let live = availability(&dom, &database(), &[model], true, model);
    assert!(live.ungroup);
}

#[test]
fn paste_is_live_exactly_while_the_clipboard_holds_something() {
    let (dom, _, _, _, loose) = place();
    let database = database();
    assert!(!availability(&dom, &database, &[loose], true, loose).paste);
    assert!(availability(&dom, &database, &[loose], false, loose).paste);
}

#[test]
fn a_row_whose_instance_is_gone_offers_no_delete() {
    let (mut dom, _, _, inside, loose) = place();
    dom.remove(inside);
    let live = availability(&dom, &database(), &[loose], true, inside);
    assert!(!live.delete);
    assert!(!live.rename);
}

#[test]
fn a_service_cannot_be_renamed_but_an_instance_under_it_can() {
    let (dom, workspace, _, _, loose) = place();
    let database = database();
    assert!(!renameable(&dom, &database, workspace));
    assert!(renameable(&dom, &database, loose));
}
