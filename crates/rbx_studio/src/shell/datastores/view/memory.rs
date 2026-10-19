//! The Memory Store section: sorted maps and queues.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::super::super::state::{ListId, Load, MemoryKind, TreeTarget, ValueView};
use super::super::super::DataStoresWindow;
use super::super::common::{
    column_label, json_area, list_row, list_state, load_more, time_label, value_view,
};
use super::super::entry::boxed;
use super::{segment, side_column, Click, SideColumn};

#[path = "queue.rs"]
mod queue;

fn sort_label(key: &rbx_cloud::SortKey) -> String {
    match key {
        rbx_cloud::SortKey::Text(text) => text.clone(),
        rbx_cloud::SortKey::Number(n) => n.to_string(),
    }
}

pub(super) fn error_line(error: &Option<String>) -> Option<Div> {
    error
        .as_ref()
        .map(|e| div().text_color(tokens::text_error()).child(e.clone()))
}

impl DataStoresWindow {
    pub(in super::super) fn memory_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let queues = self.memory_kind == MemoryKind::Queue;
        let tabs = h_flex()
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(tokens::border())
            .child(segment(
                ["ds-mem-maps", "ds-mem-queues"],
                ["Sorted maps", "Queues"],
                !queues,
                Box::new(
                    cx.listener(|this, _, _, cx| this.set_memory_kind(MemoryKind::SortedMap, cx)),
                ),
                Box::new(cx.listener(|this, _, _, cx| this.set_memory_kind(MemoryKind::Queue, cx))),
            ));
        let content = if queues {
            self.queue_section(cx)
        } else {
            self.map_section(cx)
        };
        v_flex()
            .size_full()
            .min_h_0()
            .child(tabs)
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }

    fn map_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let recent = self
            .map
            .recent
            .iter()
            .cloned()
            .map(|name| {
                let n = name.clone();
                let click: Click =
                    Box::new(cx.listener(move |this, _, _, cx| this.open_map_named(n.clone(), cx)));
                (name, click)
            })
            .collect();
        let side = side_column(SideColumn {
            title: "SORTED MAP",
            fields: vec![("Name", self.inputs.map_name.clone())],
            open: Box::new(cx.listener(|this, _, _, cx| this.open_map(cx))),
            note: "Open Cloud can\u{2019}t list sorted maps, so open one by name.",
            recent,
        });
        let main = if self.map.name.is_some() {
            self.map_main(cx)
        } else {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(tokens::text3())
                .child("Open a sorted map to see its items.")
                .into_any_element()
        };
        let detail = self.map.name.is_some().then(|| self.map_detail(cx));
        h_flex()
            .size_full()
            .child(side)
            .child(main)
            .children(detail)
            .into_any_element()
    }

    fn map_main(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let descending = self.map.descending;
        let toolbar = h_flex()
            .px(px(12.))
            .py(px(8.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(tokens::border())
            .child(segment(
                ["ds-map-asc", "ds-map-desc"],
                ["A \u{2192} Z", "Z \u{2192} A"],
                !descending,
                Box::new(cx.listener(|this, _, _, cx| this.set_map_descending(false, cx))),
                Box::new(cx.listener(|this, _, _, cx| this.set_map_descending(true, cx))),
            ))
            .child(boxed(&self.inputs.map_after, 160.))
            .child(
                ui::button("ds-map-apply", "Apply", Weight::Secondary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.load_map(false, cx))),
            )
            .child(div().flex_1())
            .child(
                ui::icon_button("ds-map-new", "plus", "New item", Weight::Secondary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.start_map_edit(true, cx))),
            );
        let mut list = v_flex();
        for (i, item) in self.map.list.items.iter().enumerate() {
            let selected = self.map.list.selected == Some(i);
            list = list.child(
                list_row(SharedString::from(format!("ds-mapitem-{i}")), selected)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(tokens::font_mono())
                            .text_size(px(12.5))
                            .child(item.key.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(tokens::text3())
                            .child(item.sort_key.as_ref().map(sort_label).unwrap_or_default()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(tokens::text3())
                            .child(time_label(item.expire_time.as_deref())),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.select_row(ListId::MapItems, i, cx)),
                    ),
            );
        }
        let has_items = !self.map.list.items.is_empty();
        let state = list_state(&self.map.list.load, has_items, "No items in this map.", cx);
        let more = self.map.list.next.is_some().then(|| {
            load_more(
                "ds-map-more",
                self.map.list.load == Load::Busy,
                cx.listener(|this, _, _, cx| this.load_map(true, cx)),
            )
        });
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(toolbar)
            .child(
                v_flex()
                    .id("ds-map-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list)
                    .children(state)
                    .children(more),
            )
            .into_any_element()
    }

    fn map_detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let pane = v_flex()
            .id("ds-map-detail")
            .w(px(340.))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .gap(px(8.))
            .p(px(12.))
            .border_l_1()
            .border_color(tokens::border());
        if let Some(creating) = self.map.creating {
            return pane
                .child(column_label(if creating {
                    "NEW ITEM"
                } else {
                    "EDIT ITEM"
                }))
                .when(creating, |d| {
                    d.child(div().text_color(tokens::text2()).child("Key"))
                        .child(boxed(&self.inputs.map_key, 316.))
                })
                .child(div().text_color(tokens::text2()).child("Value (JSON)"))
                .child(json_area(&self.inputs.map_value))
                .child(
                    div()
                        .text_color(tokens::text2())
                        .child("Expires after (seconds, max 45 days)"),
                )
                .child(boxed(&self.inputs.map_ttl, 316.))
                .child(
                    div()
                        .text_color(tokens::text2())
                        .child("Sort key (number or text, optional)"),
                )
                .child(boxed(&self.inputs.map_sort_key, 316.))
                .children(error_line(&self.map.error))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(
                            ui::button("ds-map-cancel", "Cancel", Weight::Ghost, true)
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                        )
                        .child(
                            ui::button(
                                "ds-map-save",
                                "Review and save\u{2026}",
                                Weight::Primary,
                                true,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.submit_map_edit(cx))),
                        ),
                )
                .into_any_element();
        }
        let item = self
            .map
            .list
            .selected
            .and_then(|i| self.map.list.items.get(i))
            .cloned();
        let Some(item) = item else {
            return pane
                .items_center()
                .justify_center()
                .text_color(tokens::text3())
                .child("Select an item to see its value.")
                .into_any_element();
        };
        pane.child(
            div()
                .font_family(tokens::font_mono())
                .font_weight(FontWeight::BOLD)
                .child(item.key.clone()),
        )
        .child(div().text_color(tokens::text3()).child(format!(
            "Expires {}",
            time_label(item.expire_time.as_deref())
        )))
        .child(
            h_flex()
                .gap(px(8.))
                .child(
                    ui::icon_button("ds-map-edit", "pencil", "Edit", Weight::Secondary, true)
                        .on_click(cx.listener(|this, _, _, cx| this.start_map_edit(false, cx))),
                )
                .child(
                    ui::icon_button("ds-map-del", "trash-2", "Delete", Weight::Danger, true)
                        .on_click(cx.listener(|this, _, _, cx| this.ask_map_delete(cx))),
                ),
        )
        .child(value_view(
            &item.value,
            &self.map.collapsed,
            ValueView::Tree,
            TreeTarget::MapItem,
            cx,
        ))
        .into_any_element()
    }
}
