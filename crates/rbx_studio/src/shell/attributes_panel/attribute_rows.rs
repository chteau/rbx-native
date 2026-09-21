//! The Attributes section's rows: one per attribute (a name, rename/remove
//! buttons and the value control from [`super::attribute_value`]), plus the
//! section's own "add attribute" row.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectState};
use gpui_kit::component::{h_flex, v_flex, IndexPath, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};

use crate::properties::attributes as attrs;
use crate::shell::chrome;
use crate::shell::rows::{field_box, name_indent, row_frame, section_header, select_field};
use crate::shell::Shell;
use crate::tokens;

use super::{Renaming, ATTRIBUTES_CATEGORY};

impl Shell {
    /// A row becomes whichever widget its `EditKind` calls for (see
    /// `shell::panels::properties`, which forces every section open so a
    /// filter match is never hidden behind a collapsed one — this section
    /// follows the same rule, filtering by attribute name through the same
    /// [`crate::properties::matches`] the ordinary property rows use).
    /// The "add attribute" row is never filtered out: it names nothing yet.
    pub(super) fn attribute_section(
        &mut self,
        reference: Ref,
        filter: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let filtering = !filter.trim().is_empty();
        let open = filtering || !self.is_category_collapsed(ATTRIBUTES_CATEGORY);
        let current = attrs::attributes_matching(&self.dom, reference, filter);
        let error = self.attribute_edits.attribute_error.clone();

        let mut rows = Vec::with_capacity(current.len() + 1);
        for (name, value) in &current {
            rows.push(self.attribute_row(name, value, window, cx));
        }
        rows.push(self.add_attribute_row(reference, window, cx));

        let handle = cx.entity();
        v_flex()
            .w_full()
            .child(self.properties_nav.claim(
                section_header(SharedString::from(ATTRIBUTES_CATEGORY), open, {
                    let handle = handle.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.toggle_category(ATTRIBUTES_CATEGORY, cx)
                        });
                    }
                }),
                cx,
            ))
            .when(open, |this| {
                this.child(
                    v_flex()
                        .w_full()
                        .pt(tokens::section_gap())
                        .pb(tokens::group_gap())
                        .gap(tokens::row_gap())
                        .children(rows)
                        .when_some(error, |this, message| {
                            this.child(
                                div()
                                    .px(tokens::row_padding())
                                    .text_size(tokens::text_sm())
                                    .line_height(tokens::line_sm())
                                    .text_color(tokens::text_error())
                                    .child(SharedString::from(message)),
                            )
                        }),
                )
            })
    }

    /// One attribute: a name (a label, or an `Input` while a rename is in
    /// progress) plus rename/remove buttons on one line, its value's own
    /// per-type editor on the next — the same stacked shape a composite
    /// property row already uses (see `shell::rows::property_stack`), kept
    /// uniform here across every type rather than switching layouts, since
    /// this section is visually its own rather than one more property row.
    fn attribute_row(
        &mut self,
        name: &str,
        value: &rbx_dom::Variant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let renaming = matches!(&self.attribute_edits.renaming, Some(r) if r.old_name == name);

        let name_widget: AnyElement = if renaming {
            let input = self
                .attribute_edits
                .renaming
                .as_ref()
                .expect("just matched Some above")
                .input
                .clone();
            field_box()
                .flex_1()
                .child(
                    Input::new(&input)
                        .appearance(false)
                        .with_size(tokens::field_size())
                        .h_full(),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .truncate()
                .text_color(tokens::text_muted())
                .child(SharedString::from(name.to_owned()))
                .into_any_element()
        };

        let handle = cx.entity();
        let rename_name = name.to_owned();
        let remove_name = name.to_owned();

        let header = h_flex()
            .w_full()
            .items_center()
            .gap(tokens::label_gap())
            .child(name_widget)
            .child(
                self.properties_nav.claim(
                    chrome::icon_button(
                        SharedString::from(format!("attr-rename-{name}")),
                        IconName::Pencil,
                        "Rename attribute",
                    )
                    .on_click({
                        let handle = handle.clone();
                        move |_, window, cx| {
                            let name = rename_name.clone();
                            handle.update(cx, |shell, cx| {
                                shell.begin_rename_attribute(name, window, cx);
                            });
                        }
                    }),
                    cx,
                ),
            )
            .child(
                self.properties_nav.claim(
                    chrome::icon_button(
                        SharedString::from(format!("attr-remove-{name}")),
                        IconName::Trash,
                        "Remove attribute",
                    )
                    .on_click(move |_, _, cx| {
                        let name = remove_name.clone();
                        handle.update(cx, |shell, cx| shell.remove_attribute(&name, cx));
                    }),
                    cx,
                ),
            );

        let control = self.attribute_value_control(name, value, window, cx);

        row_frame()
            .gap(tokens::label_gap())
            // The same name column every property row starts in, so the
            // panel has one left edge from its first row to its last
            // rather than one per section.
            .pl(name_indent(0))
            .pr(tokens::row_padding())
            .rounded(tokens::RADIUS)
            .hover(|this| this.bg(tokens::hover()))
            .child(header)
            .child(control)
            .into_any_element()
    }

    /// The section's own "add attribute" row: a name field, a type picker
    /// (`properties::attributes::ATTRIBUTE_TYPES`) and a confirm button —
    /// Roblox's own popup asks for the same two things (`studio/properties.md`:
    /// "enter the attribute Name, select its Type, and click Save").
    fn add_attribute_row(
        &mut self,
        reference: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name_input = match &self.attribute_edits.new_name {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("New attribute"));
                let subscription = cx.subscribe(&input, move |shell, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        shell.commit_add_attribute(cx);
                    }
                });
                self.attribute_edits.new_name = Some(input.clone());
                self.attribute_edits._subscriptions.push(subscription);
                input
            }
        };
        let type_select = match &self.attribute_edits.new_type {
            Some(select) => select.clone(),
            None => {
                let options = SearchableVec::new(
                    attrs::ATTRIBUTE_TYPES
                        .iter()
                        .map(|name| SharedString::from(*name))
                        .collect::<Vec<_>>(),
                );
                let select =
                    cx.new(|cx| SelectState::new(options, Some(IndexPath::new(0)), window, cx));
                self.attribute_edits.new_type = Some(select.clone());
                select
            }
        };

        let handle = cx.entity();
        h_flex()
            .w_full()
            .items_center()
            .gap(tokens::label_gap())
            .pl(name_indent(0))
            .pr(tokens::row_padding())
            .child(
                field_box().flex_1().child(
                    Input::new(&name_input)
                        .appearance(false)
                        .with_size(tokens::field_size())
                        .h_full()
                        .tab_index(self.tab_order.next()),
                ),
            )
            .child(
                // No `.tab_index` here: the toolkit's `Select` exposes none
                // to set (see `UX_GUIDELINES.md` §1's Level A keyboard gap) —
                // the same reason `shell::rows::render_editor`'s own
                // `RowEditor::Enum` arm never sets one either.
                select_field(&type_select.read(cx).focus_handle(cx), window, cx)
                    .w(px(120.))
                    .child(
                        Select::new(&type_select)
                            .appearance(false)
                            .with_size(tokens::field_size())
                            .h_full()
                            .py_0()
                            .pt(tokens::select_inset()),
                    ),
            )
            .child(
                self.properties_nav.claim(
                    chrome::icon_button(
                        SharedString::from(format!("attr-add-{}", reference.value())),
                        IconName::Plus,
                        "Add attribute",
                    )
                    .on_click(move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.commit_add_attribute(cx));
                    }),
                    cx,
                ),
            )
            .into_any_element()
    }

    fn begin_rename_attribute(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name.clone()));
        let subscription = cx.subscribe(&input, move |shell, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                shell.commit_rename_attribute(cx);
            }
        });
        self.attribute_edits.renaming = Some(Renaming {
            old_name: name,
            input,
            _subscription: subscription,
        });
        self.attribute_edits.attribute_error = None;
        cx.notify();
    }

    fn commit_rename_attribute(&mut self, cx: &mut Context<Self>) {
        let Some(renaming) = &self.attribute_edits.renaming else {
            return;
        };
        let old_name = renaming.old_name.clone();
        let new_name = renaming.input.read(cx).value().trim().to_owned();
        let Some(reference) = self.selected() else {
            return;
        };

        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = attrs::rename_attribute(&mut dom, reference, &old_name, &new_name);
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);

        match result {
            Ok(()) => {
                self.attribute_edits.renaming = None;
                self.attribute_edits.attribute_error = None;
            }
            Err(message) => self.attribute_edits.attribute_error = Some(message),
        }
        cx.notify();
    }

    fn remove_attribute(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(reference) = self.selected() else {
            return;
        };
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = attrs::remove_attribute(&mut dom, reference, name);
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);

        match result {
            Ok(()) => self.attribute_edits.attribute_error = None,
            Err(message) => self.attribute_edits.attribute_error = Some(message),
        }
        cx.notify();
    }

    fn commit_add_attribute(&mut self, cx: &mut Context<Self>) {
        let Some(reference) = self.selected() else {
            return;
        };
        let Some(name_input) = self.attribute_edits.new_name.clone() else {
            return;
        };
        let Some(type_select) = self.attribute_edits.new_type.clone() else {
            return;
        };
        let name = name_input.read(cx).value().trim().to_owned();
        let Some(type_name) = type_select.read(cx).selected_value().cloned() else {
            return;
        };
        let Some(value) = attrs::default_value(&type_name) else {
            return;
        };

        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = attrs::add_attribute(&mut dom, reference, &name, value);
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);

        match result {
            Ok(()) => {
                self.attribute_edits.attribute_error = None;
                // Dropped rather than cleared in place: the next render's
                // `add_attribute_row` sees `None` and builds a fresh, empty
                // pair exactly the way it already does the first time this
                // row ever renders — no second code path, and no `Window`
                // needed here to call `InputState::set_value` (this method
                // only ever runs from a `PressEnter` subscription or a plain
                // click handler, neither of which hands one over).
                self.attribute_edits.new_name = None;
                self.attribute_edits.new_type = None;
            }
            Err(message) => self.attribute_edits.attribute_error = Some(message),
        }
        cx.notify();
    }
}
