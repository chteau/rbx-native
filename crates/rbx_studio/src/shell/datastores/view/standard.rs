//! The Data Stores section: the stores column and the keys column, beside
//! the entry pane.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::super::super::rows;
use super::super::state::ListId;
use super::super::DataStoresWindow;
use super::common::{column_label, list_row, list_state, load_more, search_field};
use super::entry::boxed;

impl DataStoresWindow {
    pub(super) fn standard_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let stores = self.stores_column(cx);
        let keys = self.keys_column(cx);
        let entry = self.entry_pane(cx);
        h_flex()
            .size_full()
            .min_h_0()
            .child(stores)
            .child(keys)
            .child(entry)
            .into_any_element()
    }

    fn stores_column(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = v_flex();
        for (i, info) in self.stores.items.iter().enumerate() {
            let selected = self.store.as_deref() == Some(info.id.as_str());
            let deleted = info.deleted;
            list = list.child(
                list_row(SharedString::from(format!("ds-store-{i}")), selected)
                    .child(ui::icon("database", 13.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .when(deleted, |d| d.text_color(tokens::text3()))
                            .child(info.id.clone()),
                    )
                    .when(deleted, |d| {
                        d.child(ui::tag("Deleted", tokens::text3(), tokens::hover()))
                    })
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.select_row(ListId::Stores, i, cx)),
                    ),
            );
        }
        let has_items = !self.stores.items.is_empty();
        let state = list_state(
            &self.stores.load,
            has_items,
            "No data stores yet. Game servers create them on first write.",
            cx,
        );
        let more = self.stores.next.is_some().then(|| {
            load_more(
                "ds-stores-more",
                self.stores.load == super::super::state::Load::Busy,
                cx.listener(|this, _, _, cx| this.load_more_stores(cx)),
            )
        });
        let show_deleted = self.show_deleted;
        v_flex()
            .w(px(240.))
            .h_full()
            .flex_none()
            .bg(tokens::black())
            .border_r_1()
            .border_color(tokens::border())
            .child(
                h_flex()
                    .px(px(12.))
                    .py(px(8.))
                    .gap(px(6.))
                    .child(column_label("DATA STORES"))
                    .child(column_label(self.stores.items.len().to_string())),
            )
            .child(search_field(&self.inputs.store_search, "search"))
            .child(
                h_flex()
                    .px(px(12.))
                    .py(px(8.))
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_color(tokens::text2())
                            .child("Show deleted"),
                    )
                    .child(rows::checkbox(
                        "ds-show-deleted",
                        show_deleted,
                        cx.listener(move |this, _, _, cx| this.set_show_deleted(!show_deleted, cx)),
                    )),
            )
            .child(
                v_flex()
                    .id("ds-stores-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list)
                    .children(state)
                    .children(more),
            )
            .into_any_element()
    }

    fn keys_column(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let column = v_flex()
            .w(px(300.))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(tokens::border());
        let Some(store) = self.store.clone() else {
            return column
                .items_center()
                .justify_center()
                .text_color(tokens::text3())
                .child("Pick a data store.")
                .into_any_element();
        };
        let all_scopes = self.inputs.scope.read(cx).value() == rbx_cloud::ALL_SCOPES;
        let prefix = self.inputs.key_search.read(cx).value().to_string();
        let empty = if prefix.is_empty() {
            "No keys".to_string()
        } else {
            format!("No keys start with \"{prefix}\"")
        };
        let mut list = v_flex();
        for (i, entry) in self.keys.items.iter().enumerate() {
            let selected = self.keys.selected == Some(i);
            list = list.child(
                list_row(SharedString::from(format!("ds-key-{i}")), selected)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(tokens::font_mono())
                            .text_size(px(12.5))
                            .child(entry.key.clone()),
                    )
                    .when(all_scopes, |d| {
                        d.child(ui::tag("scope", tokens::text3(), tokens::hover()))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(tokens::text3())
                                    .child(entry.scope.clone()),
                            )
                    })
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.select_row(ListId::Keys, i, cx)),
                    ),
            );
        }
        let has_items = !self.keys.items.is_empty();
        let state = list_state(&self.keys.load, has_items, &empty, cx);
        let more = self.keys.next.is_some().then(|| {
            load_more(
                "ds-keys-more",
                self.keys.load == super::super::state::Load::Busy,
                cx.listener(|this, _, _, cx| this.load_more_keys(cx)),
            )
        });
        column
            .child(
                h_flex()
                    .px(px(12.))
                    .py(px(8.))
                    .gap(px(6.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(column_label(format!("KEYS \u{b7} {store}"))),
                    )
                    .child(
                        ui::icon_button(
                            "ds-new-entry",
                            "plus",
                            "New entry",
                            Weight::Secondary,
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.start_edit(true, cx))),
                    ),
            )
            .child(
                h_flex()
                    .px(px(12.))
                    .pb(px(8.))
                    .gap(px(8.))
                    .items_center()
                    .child(div().text_color(tokens::text2()).child("Scope"))
                    .child(boxed(&self.inputs.scope, 120.))
                    .child(div().flex_1())
                    .child(div().text_color(tokens::text2()).child("All scopes"))
                    .child(rows::checkbox(
                        "ds-all-scopes",
                        all_scopes,
                        cx.listener(move |this, _, _, cx| this.set_all_scopes(!all_scopes, cx)),
                    )),
            )
            .child(search_field(&self.inputs.key_search, "key-round"))
            .child(
                v_flex()
                    .id("ds-keys-scroll")
                    .flex_1()
                    .min_h_0()
                    .mt(px(8.))
                    .overflow_y_scroll()
                    .child(list)
                    .children(state)
                    .children(more),
            )
            .into_any_element()
    }
}
