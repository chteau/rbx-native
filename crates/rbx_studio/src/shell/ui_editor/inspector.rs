//! The canvas sidebar's design fields, Figma's: a `GuiObject`'s position,
//! size, rotation, opacity, corner radius, fill, stroke, auto layout and
//! constraints as compact fields that write straight through
//! `Shell::write_drag` — one undo step per typed value, per colour picked,
//! per drag of a field's label.
//!
//! Where Roblox keeps the look on a modifier rather than on the element —
//! a `UICorner`'s radius, a `UIStroke`, a `UIListLayout`'s gap — the field
//! reads that child, and an edit makes it when it is missing: a rounded
//! corner is one drag, not an insert, a select and a property row.
//!
//! Every value goes in as `properties::edit::commit` text, read back with
//! `properties::edit::edit_text`, so a field can never disagree with the
//! Properties panel's row for the same value.

mod actions;
mod sections;
mod spec;
mod value;
mod view;

use std::collections::HashMap;

use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_dom::{Change, Ref};

use super::super::edit::{hsla_to_rgb, resync_field, rgb_to_hsla};
use super::super::scrub::precision;
use super::{is_gui_object, Shell};
use crate::ui_canvas::round_scale;
use rbx_viewer::snap::round_to;
use spec::Form;
pub(super) use spec::{Key, On, CORNERS};
use value::{numbers_of, parse, read, show, write};

/// A label drag in flight.
struct Drag {
    key: Key,
    /// Where the label was grabbed, in window pixels — `None` for a drag
    /// the canvas drives (a radius handle, a band), which the window's own
    /// mouse move must leave alone.
    origin: Option<f32>,
    start: f32,
    last: f32,
    first: bool,
    /// The log of the step that made the modifier the drag writes to, when
    /// it did — see `Shell::write_drag_after`.
    opened: Vec<Change>,
}

/// The fields' own editing state, kept between frames and dropped on every
/// selection change — a half-typed value must not land on what is
/// selected next.
#[derive(Default)]
pub(super) struct Inspector {
    inputs: HashMap<Key, (Entity<InputState>, Subscription)>,
    colors: HashMap<Key, (Entity<ColorPickerState>, Subscription)>,
    drag: Option<Drag>,
    /// Whether the four corners' own radii are broken out.
    corners: bool,
}

impl Inspector {
    pub(super) fn clear(&mut self) {
        self.inputs.clear();
        self.colors.clear();
        self.drag = None;
    }
}

impl Shell {
    /// The selected `GuiObject`s: what every field reads and writes.
    pub(super) fn inspected(&self) -> Vec<Ref> {
        self.selected_all()
            .iter()
            .copied()
            .filter(|&r| is_gui_object(&self.dom, &self.database, r))
            .collect()
    }

    fn anchor_is(&self, class: &str) -> bool {
        self.inspected().first().is_some_and(|&r| {
            self.dom
                .get(r)
                .is_some_and(|i| self.database.is_subclass_of(i.class(), class))
        })
    }

    /// The first inspected element's child of `class`, if it has one.
    pub(super) fn anchor_child(&self, class: &str) -> Option<Ref> {
        self.child_of(*self.inspected().first()?, class)
    }

    pub(super) fn child_of(&self, element: Ref, class: &str) -> Option<Ref> {
        let instance = self.dom.get(element)?;
        instance
            .children()
            .iter()
            .copied()
            .find(|&child| self.dom.get(child).is_some_and(|i| i.class() == class))
    }

    /// `property`'s numbers on `referent` — its own value, or its class's
    /// default where it stores none.
    fn numbers(&self, referent: Ref, property: &str) -> Option<Vec<f32>> {
        let instance = self.dom.get(referent)?;
        // A `UICorner` saved before the per-corner radii keeps only its
        // `CornerRadius`, which is what draws while none of them is stored.
        let stored = |name: &str| instance.properties().contains_key(name);
        let corners = [
            "TopLeftRadius",
            "TopRightRadius",
            "BottomRightRadius",
            "BottomLeftRadius",
        ];
        let property = match corners.contains(&property)
            && !corners.iter().any(|&corner| stored(corner))
            && stored("CornerRadius")
        {
            true => "CornerRadius",
            false => property,
        };
        let (_, value) = self.database.stored_or_default(instance, property)?;
        numbers_of(value)
    }

    /// What `key` reads across the selection: `None` with nothing to read
    /// it from, `Some(None)` where the selection disagrees.
    pub(super) fn reading(&self, key: Key) -> Option<Option<Vec<f32>>> {
        let spec = self.spec(key);
        let mut readings = self.inspected().into_iter().filter_map(|element| {
            let target = match spec.on {
                On::Own => element,
                // No `UICorner` is a square corner, no `UIPadding` no
                // padding: fields an edit makes the modifier for read 0.
                On::Child(class)
                    if key.made_on_edit() && self.child_of(element, class).is_none() =>
                {
                    return Some(vec![0.0])
                }
                On::Child(class) => self.child_of(element, class)?,
            };
            let numbers = self.numbers(target, spec.properties[0])?;
            read(spec.form, spec.parts[0], &numbers)
        });
        let first = readings.next()?;
        Some(readings.all(|other| other == first).then_some(first))
    }

    /// The writes that put `value` — in the field's own terms — on every
    /// inspected element's `key`, and the elements that need the modifier
    /// it lives on made first.
    fn key_writes(&self, key: Key, value: &[f32]) -> (super::tree::Writes, Vec<Ref>) {
        let spec = self.spec(key);
        let mut writes = Vec::new();
        let mut missing = Vec::new();
        for element in self.inspected() {
            let target = match spec.on {
                On::Own => element,
                On::Child(class) => match self.child_of(element, class) {
                    Some(child) => child,
                    None => {
                        missing.push(element);
                        continue;
                    }
                },
            };
            for &property in spec.properties {
                if let Some(numbers) = self.numbers(target, property) {
                    writes.push((target, property, write(spec, numbers, value)));
                }
            }
        }
        (writes, missing)
    }

    /// Writes `value` to `key` — the step of a drag, or with `first` an edit
    /// of its own — making the modifier it lives on wherever it is missing.
    fn commit_key(&mut self, key: Key, value: Vec<f32>, first: bool, cx: &mut Context<Self>) {
        let (writes, missing) = self.key_writes(key, &value);
        let spec = self.spec(key);
        let class = match spec.on {
            On::Child(class) if !missing.is_empty() => class,
            _ => {
                let opened = self
                    .ui
                    .inspector
                    .drag
                    .as_ref()
                    .map(|drag| drag.opened.clone())
                    .unwrap_or_default();
                self.write_drag_after(first, &writes, &opened, cx);
                return;
            }
        };
        let fresh: Vec<(&'static str, String)> = spec
            .properties
            .iter()
            .filter_map(|&property| {
                let default = self.database.stored_default(class, property)?;
                Some((property, write(spec, numbers_of(default)?, &value)))
            })
            .collect();
        let opened = self.edit_gui_tree(
            class,
            |dom, _| {
                let mut all = writes;
                for element in missing {
                    let child = dom.new_instance(class, class, Some(element));
                    all.extend(actions::seed(class).map(|(p, t)| (child, p, t)));
                    all.extend(fresh.iter().map(|(p, t)| (child, *p, t.clone())));
                }
                (all, None)
            },
            cx,
        );
        if let Some(drag) = &mut self.ui.inspector.drag {
            drag.opened = opened;
        }
    }

    /// A value typed into `key`'s field, on Enter or leaving it.
    fn typed(&mut self, key: Key, text: &str, cx: &mut Context<Self>) {
        let form = self.spec(key).form;
        let Some(value) = parse(form, text) else {
            return;
        };
        if self.reading(key) == Some(Some(value.clone())) {
            return;
        }
        self.commit_key(key, value, true, cx);
    }

    /// Mouse down on `key`'s label: a drag of it scrubs the value.
    fn begin_key_drag(&mut self, key: Key, x: Pixels) {
        let Some(Some(value)) = self.reading(key) else {
            return;
        };
        let (Some(&start), false) = (value.first(), self.spec(key).form == Form::Hex) else {
            return;
        };
        self.ui.inspector.drag = Some(Drag {
            key,
            origin: Some(f32::from(x)),
            start,
            last: start,
            first: true,
            opened: Vec::new(),
        });
    }

    /// One step of a label drag, from the window's own mouse move — the
    /// pointer leaves the sidebar long before a long drag is over.
    pub(in crate::shell) fn drag_inspector(
        &mut self,
        x: Pixels,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some((drag, origin)) = self
            .ui
            .inspector
            .drag
            .as_ref()
            .and_then(|drag| Some((drag, drag.origin?)))
        else {
            return;
        };
        let moved = (f32::from(x) - origin) * precision(modifiers);
        let value = match self.spec(drag.key).form {
            Form::Number { step, whole: true } => (drag.start + moved * step).round(),
            // To the step: a field that moves by tenths reads in tenths,
            // not in whatever a fractional pointer position left over.
            Form::Number { step, .. } => round_scale(round_to(drag.start + moved * step, step)),
            Form::Percent => (drag.start + moved).round().clamp(0.0, 100.0),
            Form::Hex => return,
        };
        self.drag_value_to(value, cx);
    }

    /// Opens a drag of `key` from the canvas — a corner's radius, a gap, a
    /// side of the padding — from what it reads, or nothing where the
    /// modifier it lives on is not there yet. Where it starts is returned.
    pub(super) fn begin_value_drag(&mut self, key: Key) -> f32 {
        let start = match self.reading(key) {
            Some(Some(value)) => value.first().copied().unwrap_or(0.0),
            _ => 0.0,
        };
        self.ui.inspector.drag = Some(Drag {
            key,
            origin: None,
            start,
            last: start,
            first: true,
            opened: Vec::new(),
        });
        start
    }

    /// The value drag in flight, now at `value`: one step of its one undo
    /// entry, skipped where it has not moved.
    pub(super) fn drag_value_to(&mut self, value: f32, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.ui.inspector.drag else {
            return;
        };
        if value == drag.last {
            return;
        }
        let (key, first) = (drag.key, drag.first);
        drag.last = value;
        drag.first = false;
        self.commit_key(key, vec![value], first, cx);
    }

    pub(in crate::shell) fn end_inspector_drag(&mut self) {
        self.ui.inspector.drag = None;
    }

    /// `key`'s text field, made the first time it is drawn, holding what
    /// the selection reads unless it is being typed into.
    fn key_input(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let input = match self.ui.inspector.inputs.get(&key) {
            Some((input, _)) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx));
                let subscription =
                    cx.subscribe(&input, move |shell, input, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                            let text = input.read(cx).value().to_string();
                            shell.typed(key, &text, cx);
                        }
                    });
                self.ui
                    .inspector
                    .inputs
                    .insert(key, (input.clone(), subscription));
                input
            }
        };
        let form = self.spec(key).form;
        let text = match self.reading(key) {
            Some(Some(value)) => show(form, &value),
            Some(None) => "Mixed".to_owned(),
            None => String::new(),
        };
        resync_field(&input, &text, window, cx);
        input
    }

    /// `key`'s swatch, whose palette commits each colour picked.
    fn key_color(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ColorPickerState> {
        let state = match self.ui.inspector.colors.get(&key) {
            Some((state, _)) => state.clone(),
            None => {
                let state = cx.new(|cx| ColorPickerState::new(window, cx));
                let subscription =
                    cx.subscribe(&state, move |shell, _, event: &ColorPickerEvent, cx| {
                        if let ColorPickerEvent::Change(Some(color)) = event {
                            let (r, g, b) = hsla_to_rgb(*color);
                            let value = vec![f32::from(r), f32::from(g), f32::from(b)];
                            if shell.reading(key) != Some(Some(value.clone())) {
                                shell.commit_key(key, value, true, cx);
                            }
                        }
                    });
                self.ui
                    .inspector
                    .colors
                    .insert(key, (state.clone(), subscription));
                state
            }
        };
        if let Some(Some(rgb)) = self.reading(key) {
            let rgb: Vec<u8> = rgb
                .iter()
                .map(|channel| channel.round().clamp(0.0, 255.0) as u8)
                .collect();
            let picker = state.read(cx);
            let stale = picker.value().map(hsla_to_rgb) != Some((rgb[0], rgb[1], rgb[2]));
            if stale && !picker.is_open() {
                let value = rgb_to_hsla(rgb[0], rgb[1], rgb[2]);
                state.update(cx, |state, cx| state.set_value(value, window, cx));
            }
        }
        state
    }
}

#[cfg(test)]
#[path = "inspector/tests.rs"]
mod tests;
