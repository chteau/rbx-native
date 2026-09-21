//! Routing one attribute's current value through the Properties panel's
//! existing per-type editors — the piece [`super`]'s module doc calls out as
//! the one thing here that must never grow a second implementation.

use std::rc::Rc;

use gpui_kit::*;
use rbx_dom::Variant;

use crate::properties::attributes as attrs;
use crate::properties::{EditKind, PropertyRow};
use crate::shell::rows::{checkbox, render_editor, OnOpen, OnScrub};
use crate::shell::Shell;
use crate::tokens;

use super::ATTRIBUTES_CATEGORY;

impl Shell {
    /// The attribute's value, routed through the exact per-type widget an
    /// ordinary property of that type would get
    /// (`properties::attributes::edit_kind` wraps the same
    /// `properties::value_edit_kind` `Properties::edit_kind` uses) — a
    /// `Bool` becomes the panel's own checkbox exactly as it does for a real
    /// property (see `shell::panels::properties`), because a checkbox needs
    /// no persistent widget entity either way. A type with no editor at all
    /// falls back to plain text, same as the read-only column an unsupported
    /// property type gets.
    pub(super) fn attribute_value_control(
        &mut self,
        name: &str,
        value: &Variant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row_key = attrs::row_name(name);
        let Some(kind) = attrs::edit_kind(value) else {
            // Every type `Instance:SetAttribute` accepts has an editor (see
            // `properties::attributes::ATTRIBUTE_TYPES`), so this is only
            // reachable for an attribute some other tool wrote in a type it
            // does not. `{value:?}` is the same last-resort rendering
            // `Properties::format` gives an unknown value.
            return div()
                .flex_1()
                .truncate()
                .text_color(tokens::text_muted())
                .child(SharedString::from(format!("{value:?}")))
                .into_any_element();
        };

        if let EditKind::Bool(flag) = &kind {
            let flag = *flag;
            let handle = cx.entity();
            return self
                .properties_nav
                .claim(
                    checkbox(
                        SharedString::from(format!("attr-value-{name}")),
                        flag,
                        move |_, _, cx| {
                            let text = if flag { "false" } else { "true" };
                            let row_key = row_key.clone();
                            handle.update(cx, |shell, cx| shell.commit_row(&row_key, text, cx));
                        },
                    ),
                    cx,
                )
                .into_any_element();
        }

        let row = PropertyRow {
            name: row_key.clone(),
            value: String::new(),
            category: ATTRIBUTES_CATEGORY.to_owned(),
            edit: Some(kind.clone()),
            mixed: false,
        };
        let tab_index = self.tab_order.next();
        let (widget, error) = self.edit_row(&row, &kind, window, cx);

        let scrub_handle = cx.entity();
        let scrub_name = row_key.clone();
        let on_scrub: OnScrub = Rc::new(move |index, field_kind| {
            let handle = scrub_handle.clone();
            let name = scrub_name.clone();
            Box::new(move |event: &MouseDownEvent, _, cx| {
                let name = name.clone();
                handle.update(cx, |shell, cx| {
                    shell.begin_scrub(&name, index, field_kind, event.position.x, cx);
                });
            })
        });
        // Attributes are never `Faces`/`Axes` (not one of the types
        // `Instance:SetAttribute` accepts), so `RowEditor::Flags` never
        // reaches this factory — it exists only to satisfy `render_editor`'s
        // signature. Its return type is left to inference (rather than
        // spelled out on the closure) to dodge clippy's `type_complexity`.
        // A sequence attribute's row opens the same graph an ordinary
        // sequence property's does, through the same row name.
        let open_handle = cx.entity();
        let open_name = row_key.clone();
        let on_open: OnOpen = Box::new(move |_, window, cx| {
            let name = open_name.clone();
            open_handle.update(cx, |shell, cx| {
                shell.open_sequence_editor(&name, window, cx);
            });
        });
        let control = render_editor(
            tab_index,
            &self.tab_order,
            widget,
            |_index, _checked| Box::new(|_, _, _| {}),
            on_scrub,
            on_open,
            window,
            cx,
        );
        if let Some(message) = error {
            return v_flex_error(control, message);
        }
        control
    }
}

/// A control with its last commit's error underneath — the same shape
/// `shell::rows::property_row_control`'s composite path renders, kept small
/// here since this section never needs the rest of that function.
fn v_flex_error(control: AnyElement, message: String) -> AnyElement {
    gpui_kit::component::v_flex()
        .w_full()
        .gap(tokens::label_gap())
        .child(control)
        .child(
            div()
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text_error())
                .child(SharedString::from(message)),
        )
        .into_any_element()
}
