//! The panel's text fields: one cached `Input` per editable cell, and the
//! single write path every edit in the tab commits through.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use super::{Field, Shell, Target};
use crate::style_editor;

impl Shell {
    /// One cell's live `Input`, created the first time it renders and reused
    /// afterwards so a keystroke survives the panel rebuilding around it —
    /// the same cache, for the same reason, as `shell::edit::edit_row`.
    pub(super) fn style_field(
        &mut self,
        key: impl Into<SharedString>,
        seed: &str,
        placeholder: &str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let key = key.into();
        if let Some(field) = self.style_edits.fields.get(&key) {
            let input = field.input.clone();
            resync(&input, seed, window, cx);
            return input;
        }

        let seed = seed.to_owned();
        let placeholder = placeholder.to_owned();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(seed)
        });
        let committed = key.clone();
        let subscription = cx.subscribe(&input, move |shell, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                shell.commit_style_field(&committed, cx);
            }
        });
        self.style_edits.fields.insert(
            key.clone(),
            Field {
                input: input.clone(),
                target,
                _subscription: subscription,
            },
        );
        input
    }

    /// Writes one field's text, then drops its widget so the next render
    /// reseeds it from what actually landed in the DOM (a clamped colour, a
    /// rounded offset) — again the way `shell::edit::commit_row` does.
    pub(super) fn commit_style_field(&mut self, key: &SharedString, cx: &mut Context<Self>) {
        let Some(field) = self.style_edits.fields.get(key) else {
            return;
        };
        let text = field.input.read(cx).value().to_string();
        let target = field.target.clone();

        match target {
            Target::Instance { referent, property } => {
                self.apply_style_edit(
                    move |dom, database| {
                        crate::properties::edit::commit(dom, database, referent, &property, &text)
                            .map(|_| ())
                    },
                    cx,
                );
            }
            Target::RuleProperty { rule, name } => {
                self.apply_style_edit(
                    move |dom, database| {
                        style_editor::set_rule_property(dom, database, rule, &name, &text)
                    },
                    cx,
                );
            }
            Target::NewProperty { rule } => {
                if text.trim().is_empty() {
                    return;
                }
                let Some((name, value)) = text.split_once('=') else {
                    self.style_edits.error = Some(format!("{text:?} is not a Name = value pair"));
                    cx.notify();
                    return;
                };
                let (name, value) = (name.trim().to_owned(), value.trim().to_owned());
                self.apply_style_edit(
                    move |dom, database| {
                        style_editor::set_rule_property(dom, database, rule, &name, &value)
                    },
                    cx,
                );
            }
        }
        self.style_edits.fields.remove(key);
    }

    /// The one path every Style Editor write takes: `self.dom` handed out and
    /// back the way `shell::command` does it, snapshotted for undo first, and
    /// the resulting `Change` log handed to the viewport — which now rebuilds
    /// the GUI for a styling instance too (see `rbx_viewer`'s
    /// `changes::role`), so an edited rule is visible without a reload.
    pub(super) fn apply_style_edit(
        &mut self,
        edit: impl FnOnce(&mut WeakDom, &ReflectionDatabase) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = edit(&mut dom, &self.database);
        self.dom = dom;
        let changes = self.dom.take_changes();
        // A new sheet, rule or link is a new Explorer row; a selector or
        // value edit is not, so the tree is only rebuilt when the DOM
        // actually gained or lost an instance.
        let structural = changes.iter().any(|change| {
            matches!(
                change,
                rbx_dom::Change::Added(_) | rbx_dom::Change::Removed(_)
            )
        });
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if structural {
            self.rebuild_explorer(cx);
        }
        self.style_edits.error = result.err();
        cx.notify();
    }
}

/// See `shell::edit::resync_field`, which this repeats for the Style
/// Editor's own fields: a cached widget must not hide a value that changed
/// from outside it (an undo, a Command Bar script), but must not lose a
/// keystroke in progress either.
fn resync(input: &Entity<InputState>, seed: &str, window: &mut Window, cx: &mut App) {
    if input.focus_handle(cx).is_focused(window) || input.read(cx).value().as_ref() == seed {
        return;
    }
    input.update(cx, |state, cx| state.set_value(seed.to_owned(), window, cx));
}
