//! A `Ref` row's control and the pick it starts. Clicking the row (or
//! Enter/Space on it) arms a pick for that property; the next Explorer click
//! then sets it to the clicked instance instead of selecting that instance,
//! the way Studio's own panel does it (see `properties::edit::reference`).
//! Escape, a second click on the row, or a selection change (the Explorer's
//! arrows and type-ahead included, which move without picking) disarms it;
//! Delete or Backspace on the row, or its `×`, clears it to `nil`.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::properties::edit::{ref_text, NIL_REF};
use crate::properties::{EditKind, PropertyRow};
use crate::tokens;

use super::rows::select_field;
use super::Shell;

/// Which `Ref` row is waiting for an Explorer click, and the last pick that
/// was refused, by row — a `Ref` row has no field of its own to hold an error
/// the way a typed row's editor does.
#[derive(Default)]
pub(super) struct RefPick {
    armed: Option<String>,
    error: Option<(String, String)>,
}

impl RefPick {
    pub(super) fn error_for(&self, row: &str) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(name, _)| name == row)
            .map(|(_, message)| message.as_str())
    }

    pub(super) fn is_armed(&self) -> bool {
        self.armed.is_some()
    }

    /// A click on `name`'s row: arms it, or disarms it if it already was.
    /// Arming another row moves the pick there. Either way the last
    /// refusal is stale.
    fn toggle(&mut self, name: &str) {
        self.error = None;
        self.armed = match self.armed.take() {
            Some(armed) if armed == name => None,
            _ => Some(name.to_owned()),
        };
    }

    /// The armed row, disarming it.
    fn take(&mut self) -> Option<String> {
        self.armed.take()
    }
}

impl Shell {
    pub(super) fn ref_picker(
        &mut self,
        row: &PropertyRow,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + 'static {
        let armed = self.edits.ref_pick.armed.as_deref() == Some(row.name.as_str());
        let focus = self.tab_order.claim(cx);
        // Studio's own words for this moment are "Your cursor changes"; the
        // field says what the changed cursor is waiting for.
        let label = if armed {
            "Pick in Explorer…".to_owned()
        } else {
            row.value.clone()
        };
        // By referent, not by the shown name: a dangling target also reads
        // `nil` (and so does an instance named "nil"), and both still have a
        // value to clear.
        let is_nil = matches!(&row.edit, Some(EditKind::Ref(text)) if *text == ref_text(NIL_REF));

        let click = cx.entity();
        let click_name = row.name.clone();
        let key = cx.entity();
        let key_name = row.name.clone();
        let clear = cx.entity();
        let clear_name = row.name.clone();
        select_field(&focus, window, cx)
            .id(SharedString::from(format!("ref-pick-{}", row.name)))
            .track_focus(&focus)
            .gap(tokens::label_gap())
            .cursor_pointer()
            .when(armed, |this| {
                this.bg(tokens::accent_soft())
                    .border_color(tokens::accent_line())
            })
            .on_click(move |_, _, cx| {
                let name = click_name.clone();
                click.update(cx, |shell, cx| shell.toggle_ref_pick(&name, cx));
            })
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                let name = key_name.clone();
                match event.keystroke.key.as_str() {
                    "enter" | "space" => {
                        cx.stop_propagation();
                        key.update(cx, |shell, cx| shell.toggle_ref_pick(&name, cx));
                    }
                    "delete" | "backspace" => {
                        cx.stop_propagation();
                        key.update(cx, |shell, cx| shell.commit_ref(&name, NIL_REF, cx));
                    }
                    _ => {}
                }
            })
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .when(armed || is_nil, |this| {
                        this.text_color(tokens::text_placeholder())
                    })
                    .child(label),
            )
            .when(!armed && !is_nil, |this| {
                this.child(
                    div()
                        .id(SharedString::from(format!("ref-clear-{}", row.name)))
                        .flex_none()
                        .cursor_pointer()
                        .text_color(tokens::text_placeholder())
                        .hover(|this| this.text_color(tokens::text_full()))
                        .tooltip(|window, cx| super::tooltip::text("Clear (nil)", window, cx))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let name = clear_name.clone();
                            clear.update(cx, |shell, cx| shell.commit_ref(&name, NIL_REF, cx));
                        })
                        .child(Icon::new(IconName::X).size(px(10.))),
                )
            })
    }

    fn toggle_ref_pick(&mut self, name: &str, cx: &mut Context<Self>) {
        self.edits.ref_pick.toggle(name);
        cx.notify();
    }

    /// Escape's half of the pick: true when there was one to back out of.
    pub(super) fn cancel_ref_pick(&mut self) -> bool {
        self.edits.ref_pick.take().is_some()
    }

    /// A press on an Explorer row while a pick is armed (see
    /// `shell::reparent::draggable_row`): `target` becomes the value, and the
    /// selection stays what it was. False when nothing was armed, so the
    /// press selects as usual.
    pub(super) fn finish_ref_pick(&mut self, target: Ref, cx: &mut Context<Self>) -> bool {
        let Some(name) = self.edits.ref_pick.take() else {
            return false;
        };
        self.commit_ref(&name, target, cx);
        true
    }

    fn commit_ref(&mut self, name: &str, target: Ref, cx: &mut Context<Self>) {
        let result = self.apply_edit(name, &ref_text(target), true, cx);
        self.edits.ref_pick.error = result.err().map(|message| (name.to_owned(), message));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::RefPick;

    #[test]
    fn a_second_click_disarms_and_another_row_takes_the_pick() {
        let mut pick = RefPick::default();
        pick.toggle("Part0");
        assert!(pick.is_armed());
        pick.toggle("Part0");
        assert!(!pick.is_armed());

        pick.toggle("Part0");
        pick.toggle("Part1");
        assert_eq!(pick.take().as_deref(), Some("Part1"));
        // Taken once: Escape or a second Explorer press finds nothing armed.
        assert_eq!(pick.take(), None);
    }

    #[test]
    fn arming_clears_the_last_refusal() {
        let mut pick = RefPick {
            error: Some(("Part1".to_owned(), "refused".to_owned())),
            ..RefPick::default()
        };
        assert_eq!(pick.error_for("Part1"), Some("refused"));
        assert_eq!(pick.error_for("Part0"), None);
        pick.toggle("Part1");
        assert_eq!(pick.error_for("Part1"), None);
    }
}
