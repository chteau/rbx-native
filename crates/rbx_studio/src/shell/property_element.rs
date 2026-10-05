//! One Properties row as its live widget: what `Shell::properties` builds
//! for every row it lists, and what the UI editor's sidebar builds for the
//! rows it picks out — one builder, so the two can never edit a property
//! differently.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::properties::{EditKind, PropertyRow};
use crate::tokens;

use super::rows::{
    checkbox, expander, property_expandable, property_row, property_row_control, render_editor,
    text_field, OnOpen,
};
use super::Shell;

impl Shell {
    /// `row` as the widget its `EditKind` calls for (see `shell::edit`/
    /// `shell::rows::render_editor`), or the read-only text `property_row`
    /// always was. `expand` opens a numeric row's components whatever the
    /// panel's own expander says.
    pub(super) fn property_element(
        &mut self,
        row: &PropertyRow,
        expand: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !row.children.is_empty() {
            return self.parent_element(row, expand, window, cx);
        }
        match &row.edit {
            None => property_row(row).into_any_element(),
            Some(EditKind::BrickColor(number)) => {
                let current = (!row.mixed).then_some(*number);
                let control = self.brick_color_picker(&row.name, current, window, cx);
                property_row_control(row, control, false, None).into_any_element()
            }
            // A `Content` row is the picker while it names an object, or
            // while its URI field's pick is waiting for one.
            Some(kind @ (EditKind::Ref(_) | EditKind::Content { .. }))
                if !matches!(kind, EditKind::Content { object: false, .. })
                    || self.edits.ref_pick.is_armed_for(&row.name) =>
            {
                let control = self.ref_picker(row, window, cx);
                let error = self.edits.ref_pick.error_for(&row.name).map(str::to_owned);
                property_row_control(row, control, false, error.as_deref()).into_any_element()
            }
            // No persistent entity: a checkbox commits straight
            // through the same textual path (`shell::Shell::commit_row`)
            // every other widget uses, via `cx.entity()` since a
            // `Checkbox`'s `on_click` only hands back `&mut App`
            // (see `shell::dock`'s "Show all services" toggle for
            // the same pattern).
            Some(EditKind::Bool(flag)) => {
                let handle = cx.entity();
                let name = row.name.clone();
                // Mixed shows neither state, and a click turns every
                // one of them on: one value for all of them, which
                // is what an edit to a multi-selection means.
                // Studio's own choice here is not documented.
                let flag = (!row.mixed).then_some(*flag);
                let control = self.properties_nav.claim(
                    checkbox(
                        SharedString::from(format!("prop-bool-{}", row.name)),
                        flag,
                        move |_, _, cx| {
                            let text = if flag == Some(true) { "false" } else { "true" };
                            let name = name.clone();
                            handle.update(cx, |shell, cx| shell.commit_row(&name, text, cx));
                        },
                    ),
                    cx,
                );
                // The reference's PropertyRow: label left, control pushed
                // to the row's right edge.
                let control = h_flex().w_full().justify_end().pr(px(4.)).child(control);
                property_row_control(row, control, false, None).into_any_element()
            }
            Some(kind) => {
                // A `Content` row in URI mode is the plain URI field, with a
                // button beside it that arms an object pick.
                let uri;
                let (kind, object_pick) = match kind {
                    EditKind::Content { text, .. } => {
                        uri = EditKind::Text(text.clone());
                        (&uri, true)
                    }
                    kind => (kind, false),
                };
                let tab_index = self.tab_order.next();
                let (widget, error) = self.edit_row(row, kind, window, cx);
                // Starting a scrub needs the property's name and
                // which field moved; the value itself is read off
                // the field at mouse-down (see `shell::scrub`).
                let scrub_handle = cx.entity();
                let scrub_name = row.name.clone();
                let on_scrub: super::rows::OnScrub = std::rc::Rc::new(move |index, kind| {
                    let handle = scrub_handle.clone();
                    let name = scrub_name.clone();
                    Box::new(move |event: &MouseDownEvent, _, cx| {
                        let name = name.clone();
                        handle.update(cx, |shell, cx| {
                            shell.begin_scrub(&name, index, kind, event.position.x, cx);
                        });
                    })
                });

                // A numeric value keeps the ordinary name/value row
                // — the whole value in the field, the way it reads
                // in a script — and hangs its components off an
                // expander. Collapsed, those component rows are
                // never built: a `BasePart` alone carries five of
                // them, and their fields are the bulk of what this
                // panel lays out and paints every frame.
                if let Some(summary) = widget.summary().cloned() {
                    let expanded = expand || self.is_row_expanded(&row.name);
                    let fields = expanded.then(|| {
                        render_editor(
                            tab_index,
                            &self.tab_order,
                            widget,
                            // Neither shape under an expander is a
                            // flag set or a sequence, so neither
                            // handler is ever reached.
                            |_, _| Box::new(|_, _, _| {}),
                            on_scrub,
                            Box::new(|_, _, _| {}),
                            window,
                            cx,
                        )
                    });
                    let toggle = cx.entity();
                    let toggle_name = row.name.clone();
                    let expander = self.properties_nav.claim(
                        expander(&row.name, expanded, move |_, _, cx| {
                            let name = toggle_name.clone();
                            toggle.update(cx, |shell, cx| shell.toggle_row_expanded(&name, cx));
                        }),
                        cx,
                    );
                    return property_expandable(
                        expander,
                        text_field(&summary, tab_index, cx),
                        fields,
                        error.as_deref(),
                    )
                    .into_any_element();
                }

                let composite = widget.is_composite();
                // A flag's click has to rewrite the *whole* set, so
                // the row hands the renderer a factory that knows
                // the current flags and which one moved.
                let flags = match &widget {
                    super::edit::RowEditor::Flags(_, values) => values.clone(),
                    // An optional's present/absent checkbox commits
                    // through the same one-flag path, so the factory
                    // below hands it `true`/`false` unchanged.
                    super::edit::RowEditor::Optional(present, ..) => vec![*present],
                    _ => Vec::new(),
                };
                let handle = cx.entity();
                let name = row.name.clone();
                // A sequence row's click opens the graph rather
                // than committing anything; every other row shape
                // never reaches this handler.
                let open_handle = cx.entity();
                let open_name = row.name.clone();
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
                    move |index, checked| {
                        let mut next = flags.clone();
                        next[index] = !checked;
                        let text = next
                            .iter()
                            .map(|flag| flag.to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let handle = handle.clone();
                        let name = name.clone();
                        Box::new(move |_, _, cx| {
                            let name = name.clone();
                            let text = text.clone();
                            handle.update(cx, |shell, cx| shell.commit_row(&name, &text, cx));
                        })
                    },
                    on_scrub,
                    on_open,
                    window,
                    cx,
                );
                if object_pick {
                    // A refused pick has no field of its own to hold it.
                    let picked = self.edits.ref_pick.error_for(&row.name).map(str::to_owned);
                    let control = h_flex()
                        .w_full()
                        .items_center()
                        .gap(tokens::label_gap())
                        .child(div().flex_1().min_w_0().child(control))
                        .child(self.object_pick_button(&row.name, cx));
                    let error = error.or(picked);
                    return property_row_control(row, control, false, error.as_deref())
                        .into_any_element();
                }
                property_row_control(row, control, composite, error.as_deref()).into_any_element()
            }
        }
    }

    /// The URI field's way into Object mode: arms the same Explorer pick a
    /// `Ref` row's click does, and the row draws as that picker until the
    /// pick lands or is backed out of. Studio's own panel documents no
    /// object picker for a Content property (what is on record is typing an
    /// asset id into one), so there is no Studio glyph to match: it is the
    /// toolkit's locate crosshair, and the tooltip says what it does.
    fn object_pick_button(&mut self, name: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = cx.entity();
        let name = name.to_owned();
        self.properties_nav.claim(
            super::chrome::icon_button(
                SharedString::from(format!("content-object-{name}")),
                IconName::LocateFixed,
                "Pick an object in the Explorer",
            )
            .on_click(move |_, _, cx| {
                let name = name.clone();
                handle.update(cx, |shell, cx| shell.toggle_ref_pick(&name, true, cx));
            }),
            cx,
        )
    }

    /// A row with nothing to edit itself, only rows beneath it (see
    /// `PropertyRow::children`): an expander and, once it is open, each child
    /// as the ordinary row its own `EditKind` calls for. Collapsed, the
    /// children are never built — `MaterialColors` alone has 21 swatches.
    fn parent_element(
        &mut self,
        row: &PropertyRow,
        expand: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = expand || self.is_row_expanded(&row.name);
        let children = expanded.then(|| {
            let mut children = Vec::with_capacity(row.children.len());
            for child in &row.children {
                children.push(self.property_element(child, false, window, cx));
            }
            v_flex()
                .w_full()
                .pt(tokens::row_gap())
                .gap(tokens::row_gap())
                .children(children)
                .into_any_element()
        });
        let toggle = cx.entity();
        let name = row.name.clone();
        let expander = self.properties_nav.claim(
            expander(&row.name, expanded, move |_, _, cx| {
                let name = name.clone();
                toggle.update(cx, |shell, cx| shell.toggle_row_expanded(&name, cx));
            }),
            cx,
        );
        property_expandable(expander, div(), children, None).into_any_element()
    }
}
