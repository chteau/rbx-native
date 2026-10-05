//! Quick Open's instance rows: every instance in the place, by name and by
//! path — Studio's "Quickest Open in the West!" (DevForum, 2020) searches
//! "by instance name or path name".

use gpui_kit::SharedString;
use rbx_dom::{Ref, WeakDom};

use super::commands::{Command, Run};

/// How many instance rows the list draws. A place can hold tens of
/// thousands of instances and each row is a laid-out element; the best
/// matches come first, so the cut only ever drops the weakest.
// ponytail: a plain cap, not a virtualised list; switch the rows to
// `uniform_list` if scrolling past the first few hundred is ever wanted.
pub(super) const SHOWN: usize = 200;

/// Every instance under the place's top-level ones, in Explorer order
/// (depth first), each labelled by its dotted path.
pub(super) fn instances(dom: &WeakDom) -> Vec<Command> {
    let mut rows = Vec::new();
    for &root in dom.root_refs() {
        walk(dom, root, None, &mut rows);
    }
    rows
}

fn walk(dom: &WeakDom, reference: Ref, parent: Option<&str>, rows: &mut Vec<Command>) {
    let Some(instance) = dom.get(reference) else {
        return;
    };
    let name = instance.name();
    let path = match parent {
        Some(parent) => format!("{parent}.{name}"),
        None => name.to_owned(),
    };
    let label = SharedString::from(path);
    rows.push(Command {
        name: name.to_owned().into(),
        detail: Some(label.clone()),
        label: label.clone(),
        hint: None,
        run: Run::Instance(reference),
    });
    for &child in instance.children() {
        walk(dom, child, Some(&label), rows);
    }
}
