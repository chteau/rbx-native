//! The Ordered Data Stores section; the memory store lives in `memory`.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::super::state::{ListId, Load};
use super::super::DataStoresWindow;
use super::common::{column_label, list_row, list_state, load_more};
use super::entry::boxed;

#[path = "memory.rs"]
mod memory;

pub(super) type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The left column both sections share: name fields, Open, a note and the
/// names opened this session.
pub(super) struct SideColumn {
    pub(super) title: &'static str,
    pub(super) fields: Vec<(&'static str, Entity<gpui_kit::component::input::InputState>)>,
    pub(super) open: Click,
    pub(super) note: &'static str,
    pub(super) recent: Vec<(String, Click)>,
}

pub(super) fn side_column(spec: SideColumn) -> AnyElement {
    let mut column = v_flex()
        .w(px(260.))
        .h_full()
        .flex_none()
        .gap(px(8.))
        .p(px(12.))
        .bg(tokens::black())
        .border_r_1()
        .border_color(tokens::border())
        .child(column_label(spec.title));
    for (label, input) in &spec.fields {
        column = column.child(
            v_flex()
                .gap(px(4.))
                .child(div().text_color(tokens::text2()).child(*label))
                .child(boxed(input, 236.)),
        );
    }
    column = column
        .child(ui::button("ds-side-open", "Open", Weight::Primary, true).on_click(spec.open))
        .child(
            div()
                .text_size(px(12.))
                .text_color(tokens::text3())
                .child(spec.note),
        );
    if !spec.recent.is_empty() {
        column = column.child(column_label("RECENT"));
    }
    for (i, (name, click)) in spec.recent.into_iter().enumerate() {
        column = column.child(
            list_row(SharedString::from(format!("ds-recent-{i}")), false)
                .px(px(4.))
                .child(div().min_w_0().truncate().child(name))
                .on_click(click),
        );
    }
    column.into_any_element()
}

pub(super) fn segment(
    ids: [&'static str; 2],
    labels: [&'static str; 2],
    first_on: bool,
    first: Click,
    second: Click,
) -> Div {
    let weight = |on: bool| {
        if on {
            Weight::Primary
        } else {
            Weight::Secondary
        }
    };
    h_flex()
        .gap(px(2.))
        .child(ui::button(ids[0], labels[0], weight(first_on), true).on_click(first))
        .child(ui::button(ids[1], labels[1], weight(!first_on), true).on_click(second))
}

impl DataStoresWindow {
    pub(super) fn ordered_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let recent = self
            .ordered
            .recent
            .iter()
            .cloned()
            .map(|name| {
                let n = name.clone();
                let click: Click = Box::new(
                    cx.listener(move |this, _, _, cx| this.open_ordered_named(n.clone(), cx)),
                );
                (name, click)
            })
            .collect();
        let side = side_column(SideColumn {
            title: "ORDERED DATA STORE",
            fields: vec![
                ("Name", self.inputs.ordered_store.clone()),
                ("Scope", self.inputs.ordered_scope.clone()),
            ],
            open: Box::new(cx.listener(|this, _, _, cx| this.open_ordered(cx))),
            note: "Open Cloud can\u{2019}t list ordered data stores, so open one by name.",
            recent,
        });
        let main = if self.ordered.at.is_some() {
            self.ordered_main(cx)
        } else {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(tokens::text3())
                .child("Open an ordered data store to see its entries.")
                .into_any_element()
        };
        let editor = self.ordered.at.is_some().then(|| self.ordered_editor(cx));
        h_flex()
            .size_full()
            .min_h_0()
            .child(side)
            .child(main)
            .children(editor)
            .into_any_element()
    }

    fn ordered_main(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let descending = self.ordered.query.descending;
        let toolbar = h_flex()
            .px(px(12.))
            .py(px(8.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(tokens::border())
            .child(segment(
                ["ds-ord-high", "ds-ord-low"],
                ["Highest first", "Lowest first"],
                descending,
                Box::new(cx.listener(|this, _, _, cx| this.set_ordered_descending(true, cx))),
                Box::new(cx.listener(|this, _, _, cx| this.set_ordered_descending(false, cx))),
            ))
            .child(div().text_color(tokens::text2()).child("Min"))
            .child(boxed(&self.inputs.ordered_min, 90.))
            .child(div().text_color(tokens::text2()).child("Max"))
            .child(boxed(&self.inputs.ordered_max, 90.))
            .child(
                ui::button("ds-ord-apply", "Apply", Weight::Secondary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.load_ordered(false, cx))),
            )
            .child(div().flex_1())
            .child(
                ui::icon_button("ds-ord-new", "plus", "New entry", Weight::Secondary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.start_ordered_create(cx))),
            );
        let mut list = v_flex();
        for (i, entry) in self.ordered.list.items.iter().enumerate() {
            let selected = self.ordered.list.selected == Some(i);
            list = list.child(
                list_row(SharedString::from(format!("ds-ord-{i}")), selected)
                    .child(
                        div()
                            .w(px(40.))
                            .flex_none()
                            .text_color(tokens::text3())
                            .child((i + 1).to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(tokens::font_mono())
                            .text_size(px(12.5))
                            .child(entry.key.clone()),
                    )
                    .child(
                        div()
                            .font_family(tokens::font_mono())
                            .text_size(px(12.5))
                            .child(entry.value.to_string()),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.select_row(ListId::Ordered, i, cx)),
                    ),
            );
        }
        let has_items = !self.ordered.list.items.is_empty();
        let state = list_state(
            &self.ordered.list.load,
            has_items,
            "No entries in this range.",
            cx,
        );
        let more = self.ordered.list.next.is_some().then(|| {
            load_more(
                "ds-ord-more",
                self.ordered.list.load == Load::Busy,
                cx.listener(|this, _, _, cx| this.load_ordered(true, cx)),
            )
        });
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(toolbar)
            .child(
                h_flex()
                    .px(px(12.))
                    .py(px(6.))
                    .gap(px(8.))
                    .child(div().w(px(40.)).child(column_label("#")))
                    .child(div().flex_1().child(column_label("Key")))
                    .child(column_label("Value")),
            )
            .child(
                v_flex()
                    .id("ds-ord-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list)
                    .children(state)
                    .children(more),
            )
            .into_any_element()
    }

    fn ordered_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self
            .ordered
            .list
            .selected
            .and_then(|i| self.ordered.list.items.get(i))
            .map(|e| e.key.clone());
        let creating = selected.is_none();
        let key_row: AnyElement = match selected {
            Some(key) => div()
                .font_family(tokens::font_mono())
                .font_weight(FontWeight::BOLD)
                .child(key)
                .into_any_element(),
            None => boxed(&self.inputs.ordered_key, 248.).into_any_element(),
        };
        v_flex()
            .w(px(280.))
            .h_full()
            .flex_none()
            .gap(px(8.))
            .p(px(12.))
            .border_l_1()
            .border_color(tokens::border())
            .child(column_label(if creating { "NEW ENTRY" } else { "ENTRY" }))
            .child(key_row)
            .child(div().text_color(tokens::text2()).child("Value"))
            .child(boxed(&self.inputs.ordered_value, 248.))
            .child(
                ui::button(
                    "ds-ord-save",
                    if creating { "Create" } else { "Save" },
                    Weight::Primary,
                    true,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.submit_ordered_edit(creating, cx)),
                ),
            )
            .when(!creating, |d| {
                d.child(div().text_color(tokens::text2()).child("Increment by"))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(boxed(&self.inputs.increment, 120.))
                            .child(
                                ui::button("ds-ord-inc", "Increment", Weight::Secondary, true)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.ask_ordered_increment(cx)
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        ui::icon_button("ds-ord-del", "trash-2", "Delete", Weight::Danger, true)
                            .on_click(cx.listener(|this, _, _, cx| this.ask_ordered_delete(cx))),
                    )
            })
            .into_any_element()
    }
}
