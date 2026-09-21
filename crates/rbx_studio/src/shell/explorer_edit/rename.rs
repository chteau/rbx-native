//! Renaming an Explorer row in place, from `F2` or the context menu.
//!
//! The name box *is* the row: a dialog for one word would be heavier than
//! the edit, and the Properties panel's own `Name` field already covers the
//! case where you want to see the rest of the instance while you type. Both
//! commit through the same `WeakDom::set_name`, so either one is one undo
//! step and the viewport hears about it the same way.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::Sizable as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::tokens;

use super::Shell;

/// Whether an instance can be renamed at all. A service cannot: Roblox
/// creates exactly one of each and scripts reach it by class through
/// `GetService`, which is the same reason `explorer::reparent` refuses to
/// let one be dragged somewhere else. A referent that no longer resolves
/// cannot either — there is nothing left to rename.
pub(super) fn renameable(dom: &WeakDom, database: &ReflectionDatabase, reference: Ref) -> bool {
    dom.get(reference)
        .is_some_and(|instance| !database.is_service(instance.class()))
}

/// The row being renamed, and the box holding its pending name.
pub(super) struct Renaming {
    target: Ref,
    input: Entity<InputState>,
    /// Kept alive only to stay subscribed — see `shell::edit::RowEdit`'s
    /// identical convention.
    _subscription: Subscription,
}

impl Renaming {
    pub(super) fn target(&self) -> Ref {
        self.target
    }

    pub(super) fn input(&self) -> &Entity<InputState> {
        &self.input
    }
}

/// The box a row being renamed draws in place of its label. Free-standing
/// for the reason `picker::insert_button` is (see `ExplorerEdit::row_slots`).
pub(super) fn name_box(input: &Entity<InputState>) -> AnyElement {
    div()
        .id("explorer-rename")
        .flex_1()
        .min_w(px(40.))
        .h(tokens::tree_row_height())
        // The tree's own row press selects, and the Explorer's wrapper hands
        // focus back to the tree on any click inside it — which would blur
        // this box and commit the half-typed name. A click aimed at the
        // caret has to stop here.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(
            Input::new(input)
                .appearance(false)
                .with_size(tokens::field_size())
                .h_full(),
        )
        .into_any_element()
}

impl Shell {
    /// Opens the name box on `reference`, with the current name selected so
    /// the first keystroke replaces it — a rename is almost always a new
    /// name rather than an edit to the old one.
    pub(super) fn begin_rename(
        &mut self,
        reference: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !renameable(&self.dom, &self.database, reference) {
            return;
        }
        let Some(name) = self
            .dom
            .get(reference)
            .map(|instance| instance.name().to_owned())
        else {
            return;
        };

        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let subscription = cx.subscribe(&input, |shell, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                shell.commit_rename(cx);
            }
        });
        self.explorer_edit.focus_next = Some(input.clone());

        self.explorer_edit.picker = None;
        self.explorer_edit.menu = None;
        self.explorer_edit.renaming = Some(Renaming {
            target: reference,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// `F2` on the selected row.
    pub(in crate::shell) fn begin_rename_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selected) = self.selected() {
            self.begin_rename(selected, window, cx);
        }
    }

    /// Writes the typed name, as one undo step, through the same
    /// take/put-back path every other Explorer edit uses. An empty or
    /// unchanged name closes the box without touching the DOM: `Instance`
    /// has no meaningful empty name, and a rename that renames nothing must
    /// not push an undo step that undoes nothing.
    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(renaming) = self.explorer_edit.renaming.take() else {
            return;
        };
        let typed = renaming.input.read(cx).value().trim().to_owned();
        let current = self
            .dom
            .get(renaming.target)
            .map(|instance| instance.name().to_owned());
        if typed.is_empty() || current.is_none_or(|name| name == typed) {
            cx.notify();
            return;
        }

        // See `shell::history`: snapshotted before the rename below.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let renamed = dom.set_name(renaming.target, &typed).is_ok();
        self.dom = dom;
        if !renamed {
            cx.notify();
            return;
        }
        // One `Change::Property` on `Name` — the same log a Properties-panel
        // rename produces, which is what lets an open script tab's label
        // follow along without knowing where the rename came from.
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }
}
