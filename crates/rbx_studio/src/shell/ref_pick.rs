//! A `Ref` row's control and the pick it starts. Clicking the row (or
//! Enter/Space on it) arms a pick for that property; the next Explorer click
//! then sets it to the clicked instance instead of selecting that instance,
//! the way Studio's own panel does it (see `properties::edit::reference`).
//! Escape, a second click on the row, or a selection change (the Explorer's
//! arrows and type-ahead included, which move without picking) disarms it;
//! Delete or Backspace on the row, or its `×`, clears it to `nil`.
//!
//! A click in the 3D view picks too — creator-docs once spelled it out for
//! `ObjectValue.Value`: "click the object you wish to set it to within the
//! game view or Explorer window". It lands on what a viewport click would
//! select (see [`candidate`]), and the view's hover outline previews it.
//!
//! A `Content` row naming an object is this same control, and its URI
//! field's pick button arms the same pick (see `properties::edit::content`);
//! its pick commits an object, and clearing it leaves the URI field empty.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::properties::edit::{accepts_ref, held_class, object_text, ref_text, NIL_REF};
use crate::properties::{EditKind, PropertyRow};
use crate::tokens;

use super::rows::select_field;
use super::selection;
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
    /// The row under the pointer, for the empty field's "Select <Type>…".
    hovered: Option<String>,
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
        let hovered = self.edits.ref_pick.hovered.as_deref() == Some(row.name.as_str());
        let class = self
            .selected()
            .and_then(|owner| held_class(&self.dom, &self.database, owner, &row.name));
        let label = field_label(&row.value, is_nil, armed, hovered, class);

        let click = cx.entity();
        let click_name = row.name.clone();
        let key = cx.entity();
        let key_name = row.name.clone();
        let key_cleared = cleared.clone();
        let clear = cx.entity();
        let clear_name = row.name.clone();
        let hover = cx.entity();
        let hover_name = row.name.clone();
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
            .on_hover(move |&now: &bool, _, cx| {
                let name = hover_name.clone();
                hover.update(cx, |shell, cx| {
                    let pick = &mut shell.edits.ref_pick;
                    let next = match now {
                        true => Some(name),
                        false => pick.hovered.take().filter(|row| *row != name),
                    };
                    if pick.hovered != next {
                        pick.hovered = next;
                        cx.notify();
                    }
                });
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
                    .when(is_nil, |this| this.text_color(tokens::text_placeholder()))
                    .child(label),
            )
            .when(!armed && !is_nil, |this| {
                this.child(
                    clear_glyph(&row.name)
                        .tooltip(move |window, cx| super::tooltip::text(clear_tip, window, cx))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let name = clear_name.clone();
                            let text = cleared.clone();
                            clear.update(cx, |shell, cx| shell.commit_pick(&name, &text, cx));
                        }),
                )
            })
    }

    pub(super) fn toggle_ref_pick(&mut self, name: &str, content: bool, cx: &mut Context<Self>) {
        self.edits.ref_pick.toggle(name, content);
        self.sync_viewport_pick(cx);
        cx.notify();
    }

    /// Escape's half of the pick: true when there was one to back out of.
    pub(super) fn cancel_ref_pick(&mut self, cx: &mut Context<Self>) -> bool {
        let cancelled = self.edits.ref_pick.take().is_some();
        self.sync_viewport_pick(cx);
        cancelled
    }

    /// Tells the 3D view whether its next press picks rather than selects.
    /// Every path that arms or disarms ends here, a selection change too.
    pub(super) fn sync_viewport_pick(&self, cx: &mut Context<Self>) {
        let armed = self.edits.ref_pick.is_armed();
        self.viewport
            .update(cx, |viewport, cx| viewport.set_ref_picking(armed, cx));
    }

    /// What a viewport click under `hits` (nearest first) sets the armed row
    /// to; `None` for a miss, or with nothing armed.
    pub(super) fn ref_pick_candidate(&self, hits: &[Ref], cycling: bool) -> Option<Ref> {
        let name = self.edits.ref_pick.armed.as_deref()?;
        let owners = self.selected_all();
        candidate(&self.dom, &self.database, hits, owners, name, cycling)
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
        self.sync_viewport_pick(cx);
        self.commit_pick(&name, &text, cx);
        true
    }

    fn commit_pick(&mut self, name: &str, text: &str, cx: &mut Context<Self>) {
        let result = self.apply_edit(name, text, true, cx);
        self.edits.ref_pick.error = result.err().map(|message| (name.to_owned(), message));
        cx.notify();
    }
}

/// The field's text, after Studio's own widget (`InstanceRefPropertyView`):
/// an empty field reads its `InstanceRef.Selecting` string while picking and
/// its `InstanceRef.SelectInstanceType` string, naming the class the
/// property holds, while the pointer is over it; a field with a value always
/// shows it. Only the string keys are public, not their English text, so
/// "Selecting…" and "Select <Type>…" are read off the key names.
fn field_label(
    value: &str,
    is_nil: bool,
    armed: bool,
    hovered: bool,
    class: Option<&str>,
) -> String {
    match (is_nil, armed, hovered, class) {
        (true, true, ..) => "Selecting…".to_owned(),
        (true, false, true, Some(class)) => format!("Select {class}…"),
        _ => value.to_owned(),
    }
}

/// The `×` that clears a row: a small glyph in a square the pointer can
/// actually hit — WCAG 2.5.8's 24px floor, or Large Click Targets' 44, at
/// every UI scale (`tokens::hit_target`).
fn clear_glyph(row: &str) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("ref-clear-{row}")))
        .flex_none()
        .size(tokens::hit_target())
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(tokens::text_placeholder())
        .hover(|this| this.text_color(tokens::text_full()))
        .child(Icon::new(IconName::X).size(tokens::scaled(10.)))
}

/// What a click selects (`selection::from_click`: the outermost `Model`
/// plain, the part itself with `Alt`), unless the property cannot hold that
/// and can hold the part actually clicked — a `Weld.Part0` or a
/// `PrimaryPart` clicked on a part inside a model. Studio's own widget hands
/// its instance picker the property's class (`pickInstanceAsync({className})`
/// in `InstanceRefPropertyView`), so its pick only ever lands on an instance
/// of that class; how the native picker itself resolves model against part
/// is not visible from outside it.
///
/// "Can hold" asks every one of `owners`, the whole selection the pick is
/// written to: a part only the first owner would take is no better than the
/// model, since the commit refuses it for the rest either way.
fn candidate(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    hits: &[Ref],
    owners: &[Ref],
    name: &str,
    cycling: bool,
) -> Option<Ref> {
    let accepts = |target| {
        owners
            .iter()
            .all(|&owner| accepts_ref(dom, db, owner, name, target))
    };
    let clicked = selection::from_click(dom, db, hits, owners.first().copied(), cycling)?;
    let nearest = hits[0];
    Some(match accepts(clicked) || !accepts(nearest) {
        true => clicked,
        false => nearest,
    })
}

#[cfg(test)]
mod tests;
