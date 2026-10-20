//! The entry pane: the selected key's header, value, metadata, editor and
//! version history.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_cloud::Entry;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::super::json_tree;
use super::super::state::{EntryTab, Load, TreeTarget, ValueView};
use super::super::DataStoresWindow;
use super::common::{json_area_rows, problem_panel, time_label, value_view};

mod versions;

/// A single-line field in a bordered box of a fixed width.
pub(super) fn boxed(input: &Entity<InputState>, width: f32) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .px(px(6.))
        .rounded(tokens::radius())
        .border_1()
        .border_color(tokens::border())
        .bg(tokens::field_select())
        .child(
            Input::new(input)
                .appearance(false)
                .with_size(tokens::field_size())
                .h(tokens::input_height()),
        )
}

fn meta_row(label: &'static str, content: impl IntoElement) -> Div {
    h_flex()
        .gap(px(8.))
        .items_start()
        .child(
            div()
                .w(px(110.))
                .flex_none()
                .text_color(tokens::text3())
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(content))
}

fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(tokens::font_mono())
        .text_size(px(12.))
        .child(text.into())
}

fn chip(text: String) -> Div {
    mono(text)
        .px(px(6.))
        .rounded(tokens::radius())
        .bg(tokens::hover())
}

fn tab(id: &'static str, label: &'static str, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(10.))
        .py(px(6.))
        .cursor_pointer()
        .border_b_2()
        .border_color(gpui_kit::transparent_black())
        .when(selected, |d| {
            d.text_color(tokens::check_on())
                .border_color(tokens::accent_line())
        })
        .child(label)
}

impl DataStoresWindow {
    pub(super) fn entry_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let body = self.entry_content(cx);
        div()
            .id("ds-entry-pane")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p(px(16.))
            .child(body)
            .into_any_element()
    }

    fn entry_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if let Load::Failed(problem) = self.pane.load.clone() {
            return problem_panel(&problem, cx);
        }
        if self.pane.editing.as_ref().is_some_and(|d| d.creating) {
            let store = self.store.clone().unwrap_or_default();
            let scope = self.inputs.scope.read(cx).value().to_string();
            let editor = self.editor(cx);
            return v_flex()
                .gap(px(12.))
                .child(
                    v_flex()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::BOLD)
                                .child("New entry"),
                        )
                        .child(
                            div()
                                .text_color(tokens::text3())
                                .child(format!("{store} \u{203a} {scope}")),
                        ),
                )
                .child(editor)
                .into_any_element();
        }
        let Some(entry) = self.pane.entry.clone() else {
            if self.pane.load == Load::Busy {
                return ui::spinner("ds-entry-spin", 14.).into_any_element();
            }
            return div()
                .text_color(tokens::text3())
                .child("Select a key to see its value.")
                .into_any_element();
        };
        let key = entry.key.clone();
        let tab_now = self.pane.tab;
        let main = match tab_now {
            EntryTab::Value => self.value_tab(&entry, cx),
            EntryTab::Versions => self.versions_tab(cx),
        };
        let store = self.store.clone().unwrap_or_default();
        let copy_key = key.clone();
        v_flex()
            .gap(px(12.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        mono(key)
                            .min_w_0()
                            .truncate()
                            .text_size(px(15.))
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(
                        ui::icon_button("ds-copy-key", "copy", "Copy key", Weight::Ghost, true)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.copy(copy_key.clone(), "the key", cx)
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::icon_button("ds-edit", "pencil", "Edit", Weight::Secondary, true)
                            .on_click(cx.listener(|this, _, _, cx| this.start_edit(false, cx))),
                    )
                    .child(
                        ui::icon_button("ds-delete", "trash", "Delete", Weight::Danger, true)
                            .on_click(cx.listener(|this, _, _, cx| this.ask_delete(cx))),
                    ),
            )
            .child(
                div()
                    .text_color(tokens::text3())
                    .child(format!("{store} \u{203a} {}", entry.scope)),
            )
            .child(
                h_flex()
                    .border_b_1()
                    .border_color(tokens::border())
                    .child(
                        tab("ds-tab-value", "Value", tab_now == EntryTab::Value).on_click(
                            cx.listener(|this, _, _, cx| this.set_entry_tab(EntryTab::Value, cx)),
                        ),
                    )
                    .child(
                        tab("ds-tab-versions", "Versions", tab_now == EntryTab::Versions).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.set_entry_tab(EntryTab::Versions, cx)
                            }),
                        ),
                    ),
            )
            .child(main)
            .into_any_element()
    }

    fn value_tab(&mut self, entry: &Entry, cx: &mut Context<Self>) -> AnyElement {
        if self.pane.editing.is_some() {
            return self.editor(cx);
        }
        let view = self.pane.view;
        if let Some(old) = self.pane.revision.clone() {
            let rev = rbx_cloud::Revision {
                revision_id: old.revision_id.clone(),
                revision_create_time: old.revision_create_time.clone(),
                etag: old.etag.clone(),
                deleted: old.deleted,
            };
            let banner = format!(
                "Viewing the version from {} \u{2014} read-only.",
                time_label(old.revision_create_time.as_deref())
            );
            return v_flex()
                .gap(px(10.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .p(px(10.))
                        .items_center()
                        .rounded(tokens::radius())
                        .bg(ui::wash())
                        .child(div().flex_1().child(banner))
                        .child(
                            ui::button(
                                "ds-rollback-this",
                                "Roll back to this version",
                                Weight::Primary,
                                true,
                            )
                            .on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.ask_rollback(rev.clone(), cx)
                                }),
                            ),
                        )
                        .child(
                            ui::button("ds-back-current", "Back to current", Weight::Ghost, true)
                                .on_click(cx.listener(|this, _, _, cx| this.back_to_current(cx))),
                        ),
                )
                .child(value_view(
                    &old.value,
                    &self.pane.collapsed,
                    view,
                    TreeTarget::Entry,
                    cx,
                ))
                .into_any_element();
        }
        let users = if entry.users.is_empty() {
            div().text_color(tokens::text3()).child("None")
        } else {
            h_flex()
                .flex_wrap()
                .gap(px(4.))
                .children(entry.users.iter().map(|u| chip(u.to_string())))
        };
        let attributes = if entry.attributes.is_empty() {
            div().text_color(tokens::text3()).child("None")
        } else {
            h_flex().flex_wrap().gap(px(4.)).children(
                entry
                    .attributes
                    .iter()
                    .map(|(k, v)| chip(format!("{k}: {}", json_tree::short(v, 60)))),
            )
        };
        let bytes = rbx_cloud::value_bytes(&entry.value);
        let pretty = json_tree::pretty(&entry.value);
        let integer = entry.value.is_i64() || entry.value.is_u64();
        let seg = |id: &'static str, label: &'static str, mine: ValueView| {
            ui::button(
                id,
                label,
                if view == mine {
                    Weight::Secondary
                } else {
                    Weight::Ghost
                },
                true,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_value_view(mine, cx)))
        };
        v_flex()
            .gap(px(12.))
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(meta_row("Size", json_tree::size_label(bytes)))
                    .child(meta_row("Version", mono(entry.revision_id.clone())))
                    .child(meta_row("Etag", mono(entry.etag.clone())))
                    .child(meta_row(
                        "Created",
                        time_label(entry.create_time.as_deref()),
                    ))
                    .child(meta_row(
                        "Updated",
                        time_label(entry.revision_create_time.as_deref()),
                    ))
                    .child(meta_row("UserIds", users))
                    .child(meta_row("Attributes", attributes)),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(div().w(px(110.)).text_color(tokens::text3()).child("Value"))
                    .child(seg("ds-view-tree", "Tree", ValueView::Tree))
                    .child(seg("ds-view-raw", "Raw", ValueView::Raw))
                    .child(
                        ui::button("ds-copy-value", "Copy value", Weight::Ghost, true).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.copy(pretty.clone(), "the value", cx)
                            }),
                        ),
                    )
                    .when(integer, |d| {
                        d.child(div().flex_1())
                            .child(boxed(&self.inputs.increment, 80.))
                            .child(
                                ui::button("ds-increment", "Increment", Weight::Secondary, true)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.submit_increment(cx)),
                                    ),
                            )
                    }),
            )
            .child(value_view(
                &entry.value,
                &self.pane.collapsed,
                view,
                TreeTarget::Entry,
                cx,
            ))
            .into_any_element()
    }

    fn editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(draft) = self.pane.editing.clone() else {
            return div().into_any_element();
        };
        let size = json_tree::size_label(self.inputs.value.read(cx).value().len());
        let label = |text: &'static str| div().text_color(tokens::text3()).child(text);
        v_flex()
            .gap(px(8.))
            .when(draft.creating, |d| {
                let scope = self.inputs.scope.read(cx).value().to_string();
                d.child(label("Key"))
                    .child(boxed(&self.inputs.new_key, 320.))
                    .child(label("").child(format!("Scope: {scope}")))
            })
            .child(label("Value (JSON)"))
            .child(json_area_rows(&self.inputs.value, 14))
            .child(label("").child(format!("{size} of 4 MB")))
            .child(label("UserIds"))
            .child(boxed(&self.inputs.users, 320.))
            .child(label("Attributes (JSON object, 300 characters max)"))
            .child(json_area_rows(&self.inputs.attributes, 3))
            .children(
                draft
                    .error
                    .map(|e| div().text_color(tokens::text_error()).child(e)),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        ui::button("ds-edit-cancel", "Cancel", Weight::Ghost, false)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                    )
                    .child(
                        ui::button(
                            "ds-edit-save",
                            "Review and save\u{2026}",
                            Weight::Primary,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.submit_edit(cx))),
                    ),
            )
            .into_any_element()
    }
}
