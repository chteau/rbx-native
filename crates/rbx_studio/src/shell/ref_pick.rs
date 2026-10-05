//! A `Ref` row's control and the pick it starts. Clicking the row (or
//! Enter/Space on it) arms a pick for that property; the next Explorer click
//! then sets it to the clicked instance instead of selecting that instance,
//! the way Studio's own panel does it (see `properties::edit::reference`).
//! Escape, a second click on the row, or a selection change (the Explorer's
//! arrows and type-ahead included, which move without picking) disarms it;
//! Delete or Backspace on the row, or its `×`, clears it to `nil`.
//!
//! A `Content` row naming an object is this same control, and its URI
//! field's pick button arms the same pick (see `properties::edit::content`);
//! its pick commits an object, and clearing it leaves the URI field empty.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::properties::edit::{object_text, ref_text, NIL_REF};
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
    /// The armed row is a `Content` one, whose pick commits an object
    /// rather than a referent (see `properties::edit::object_text`).
    content: bool,
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

    pub(super) fn is_armed_for(&self, row: &str) -> bool {
        self.armed.as_deref() == Some(row)
    }

    /// A click on `name`'s row: arms it, or disarms it if it already was.
    /// Arming another row moves the pick there. Either way the last
    /// refusal is stale.
    fn toggle(&mut self, name: &str, content: bool) {
        self.error = None;
        self.content = content;
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
        // A `Content` row clears to none, which is the empty URI field.
        let (is_nil, content, cleared) = match &row.edit {
            Some(EditKind::Content { object, .. }) => (!object, true, String::new()),
            Some(EditKind::Ref(text)) => (*text == ref_text(NIL_REF), false, ref_text(NIL_REF)),
            _ => (false, false, ref_text(NIL_REF)),
        };
        let clear_tip = if content {
            "Clear (none)"
        } else {
            "Clear (nil)"
        };

        let click = cx.entity();
        let click_name = row.name.clone();
        let key = cx.entity();
        let key_name = row.name.clone();
        let key_cleared = cleared.clone();
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
                click.update(cx, |shell, cx| shell.toggle_ref_pick(&name, content, cx));
            })
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                let name = key_name.clone();
                match event.keystroke.key.as_str() {
                    "enter" | "space" => {
                        cx.stop_propagation();
                        key.update(cx, |shell, cx| shell.toggle_ref_pick(&name, content, cx));
                    }
                    "delete" | "backspace" => {
                        cx.stop_propagation();
                        let text = key_cleared.clone();
                        key.update(cx, |shell, cx| shell.commit_pick(&name, &text, cx));
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
                        .tooltip(move |window, cx| super::tooltip::text(clear_tip, window, cx))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let name = clear_name.clone();
                            let text = cleared.clone();
                            clear.update(cx, |shell, cx| shell.commit_pick(&name, &text, cx));
                        })
                        .child(Icon::new(IconName::X).size(tokens::scaled(10.))),
                )
            })
    }

    pub(super) fn toggle_ref_pick(&mut self, name: &str, content: bool, cx: &mut Context<Self>) {
        self.edits.ref_pick.toggle(name, content);
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
        let text = if self.edits.ref_pick.content {
            object_text(target)
        } else {
            ref_text(target)
        };
        self.commit_pick(&name, &text, cx);
        true
    }

    fn commit_pick(&mut self, name: &str, text: &str, cx: &mut Context<Self>) {
        let result = self.apply_edit(name, text, true, cx);
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
        pick.toggle("Part0", false);
        assert!(pick.is_armed());
        pick.toggle("Part0", false);
        assert!(!pick.is_armed());

        pick.toggle("Part0", false);
        pick.toggle("Part1", false);
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
        pick.toggle("Part1", false);
        assert_eq!(pick.error_for("Part1"), None);
    }

    #[test]
    fn a_content_row_remembers_its_pick_names_an_object() {
        let mut pick = RefPick::default();
        pick.toggle("TextureContent", true);
        assert!(pick.is_armed_for("TextureContent") && pick.content);
        // Moving the pick to a `Ref` row commits a referent again.
        pick.toggle("Part0", false);
        assert!(!pick.is_armed_for("TextureContent") && !pick.content);
    }
}
