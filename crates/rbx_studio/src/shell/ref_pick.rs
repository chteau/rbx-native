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

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::properties::edit::{accepts_ref, ref_text, NIL_REF};
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
        // By referent, not by the shown name: a dangling target also reads
        // `nil` (and so does an instance named "nil"), and both still have a
        // value to clear.
        let is_nil = matches!(&row.edit, Some(EditKind::Ref(text)) if *text == ref_text(NIL_REF));
        // Studio's own Properties widget (`InstanceRefPropertyView`) swaps
        // only an empty field's text for its `InstanceRef.Selecting` string
        // while picking; a field with a value keeps showing it. The armed
        // tint and the crosshair carry the mode either way.
        let label = if armed && is_nil {
            "Selecting…".to_owned()
        } else {
            row.value.clone()
        };

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
                    .when(is_nil, |this| this.text_color(tokens::text_placeholder()))
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
        self.sync_viewport_pick(cx);
        self.commit_ref(&name, target, cx);
        true
    }

    fn commit_ref(&mut self, name: &str, target: Ref, cx: &mut Context<Self>) {
        let result = self.apply_edit(name, &ref_text(target), true, cx);
        self.edits.ref_pick.error = result.err().map(|message| (name.to_owned(), message));
        cx.notify();
    }
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
mod tests {
    use rbx_dom::{Ref, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    use super::{candidate, RefPick};

    /// A house model with a handle part inside it, and a `Weld` and an
    /// `ObjectValue` beside it, all under `Workspace`.
    struct Place {
        dom: WeakDom,
        house: Ref,
        handle: Ref,
        weld: Ref,
        value: Ref,
    }

    fn place() -> Place {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let house = dom.new_instance("Model", "House", Some(workspace));
        let handle = dom.new_instance("Part", "Handle", Some(house));
        let weld = dom.new_instance("Weld", "Weld", Some(workspace));
        let value = dom.new_instance("ObjectValue", "Value", Some(workspace));
        Place {
            dom,
            house,
            handle,
            weld,
            value,
        }
    }

    /// A viewport click on `hits` while `owner`'s `name` is armed.
    fn pick(place: &Place, owner: Ref, name: &str, hits: &[Ref], alt: bool) -> Option<Ref> {
        candidate(&place.dom, &db(), hits, &[owner], name, alt)
    }

    fn db() -> ReflectionDatabase {
        ReflectionDatabase::embedded()
    }

    #[test]
    fn a_plain_click_picks_what_a_click_would_select() {
        let place = place();
        // `ObjectValue.Value` holds any instance, so the house a plain click
        // selects is what it gets — and `Alt` reaches the part, as it does
        // for a selection.
        let hits = [place.handle];
        assert_eq!(
            pick(&place, place.value, "Value", &hits, false),
            Some(place.house)
        );
        assert_eq!(
            pick(&place, place.value, "Value", &hits, true),
            Some(place.handle)
        );
    }

    #[test]
    fn a_part_typed_ref_takes_the_part_under_the_cursor_over_its_model() {
        let place = place();
        let hits = [place.handle];
        assert_eq!(
            pick(&place, place.weld, "Part0", &hits, false),
            Some(place.handle)
        );
        assert_eq!(
            pick(&place, place.house, "PrimaryPart", &hits, false),
            Some(place.handle)
        );
    }

    #[test]
    fn every_selected_owner_has_to_take_the_part() {
        let mut place = place();
        let workspace = place.dom.root_refs()[0];
        let second = place.dom.new_instance("Weld", "Weld2", Some(workspace));
        let other = place.dom.new_instance("Model", "Truck", Some(workspace));
        let hits = [place.handle];
        // Two welds both take the part.
        assert_eq!(
            candidate(
                &place.dom,
                &db(),
                &hits,
                &[place.weld, second],
                "Part0",
                false
            ),
            Some(place.handle)
        );
        // The house would take its own handle as `PrimaryPart`, the truck
        // would not: the pick stays on the house, which both then refuse,
        // rather than a part written to one model and refused by the other.
        assert_eq!(
            candidate(
                &place.dom,
                &db(),
                &hits,
                &[place.house, other],
                "PrimaryPart",
                false
            ),
            Some(place.house)
        );
    }

    #[test]
    fn the_sky_picks_nothing() {
        let place = place();
        assert_eq!(pick(&place, place.weld, "Part0", &[], false), None);
    }

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
