//! Change Class…: every selected instance turned into another class, keeping
//! what the new class can still hold, as one undo step. What survives, and
//! why the instance keeps its referent, is `crate::change_class`; the class
//! is picked in the insert picker, given another purpose (see
//! `explorer_edit::picker`).

use gpui_kit::Context;
use rbx_dom::Ref;

use crate::change_class::{self, Plan};
use crate::command_bar::Feedback;

use super::keys::class_defaults;
use super::Shell;

/// The `source` a refusal's Output row carries, where a Command Bar run's
/// carries the command that was typed.
const SOURCE: &str = "Change Class";

/// How many classes the picker suggests again from this session.
const RECENT: usize = 5;

impl Shell {
    /// What changing `referent` to `class` would do to its properties.
    fn change_class_plan(&self, referent: Ref, class: &str) -> Option<Plan> {
        let instance = self.dom.get(referent)?;
        Some(change_class::plan(
            &self.database,
            instance,
            class,
            &class_defaults(&self.database, instance.class()),
            &class_defaults(&self.database, class),
        ))
    }

    /// The picker's footer for `class` — see `change_class::summary`.
    pub(super) fn change_class_summary(&self, targets: &[Ref], class: &str) -> String {
        let (convert, _) = change_class::partition(&self.dom, &self.database, targets, class);
        let plans: Vec<Plan> = convert
            .iter()
            .filter_map(|&referent| self.change_class_plan(referent, class))
            .collect();
        change_class::summary(&plans)
    }

    /// Changes each of `targets` that can be changed to `class`, all in one
    /// undo step. A refused one — a service — is named in the Output panel:
    /// the rest of the selection did change, and one row keeping its icon
    /// among several that swapped theirs is easy to miss.
    pub(super) fn change_class(&mut self, targets: &[Ref], class: &str, cx: &mut Context<Self>) {
        if !change_class::is_target(&self.database, class) {
            return;
        }
        let (convert, refused) = change_class::partition(&self.dom, &self.database, targets, class);
        if !refused.is_empty() {
            let names: Vec<&str> = refused
                .iter()
                .filter_map(|&referent| self.dom.get(referent))
                .map(|instance| instance.name())
                .collect();
            let was = if names.len() == 1 {
                "it was"
            } else {
                "they were"
            };
            self.output.push(
                SOURCE,
                Feedback::Warning(format!(
                    "Left {} as {was}: a service's class cannot be changed",
                    names.join(", ")
                )),
            );
        }
        if convert.is_empty() {
            cx.notify();
            return;
        }

        // A script tab's typing reaches the DOM on a debounce; landing it
        // first keeps it on its own side of this step, as `Shell::undo` does.
        self.flush_script_edits(cx);
        // See `shell::history`: one snapshot, however many instances change.
        self.push_history();
        let plans: Vec<(Ref, Plan)> = convert
            .iter()
            .filter_map(|&referent| Some((referent, self.change_class_plan(referent, class)?)))
            .collect();
        for (referent, plan) in &plans {
            change_class::apply(&mut self.dom, *referent, class, plan);
        }
        let changes = self.dom.take_changes();

        // The selection is the same instances as before, so it stays as it
        // is; `reflect_changes` is what re-reads it for their new classes.
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);

        let recent = &mut self.explorer_edit.recent_classes;
        recent.retain(|used| used != class);
        recent.insert(0, class.to_owned());
        recent.truncate(RECENT);
        cx.notify();
    }
}
