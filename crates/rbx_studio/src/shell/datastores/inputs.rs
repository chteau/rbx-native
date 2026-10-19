//! The window's text fields: made once, with every change routed to
//! `DataStoresWindow::on_input`, and read or replaced by `Field`.

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::*;

use super::{DataStoresWindow, Field, Inputs};

impl Inputs {
    /// Replaces `field`'s text.
    pub(super) fn set(&self, field: Field, text: String, window: &mut Window, cx: &mut App) {
        let line = match field {
            Field::Value => Some(&self.value),
            Field::Attributes => Some(&self.attributes),
            Field::MapValue => Some(&self.map_value),
            Field::QueueData => Some(&self.queue_data),
            _ => None,
        };
        if let Some(area) = line {
            area.update(cx, |s, cx| s.set_value(text, window, cx));
            return;
        }
        let input = match field {
            Field::StoreSearch => &self.store_search,
            Field::Scope => &self.scope,
            Field::KeySearch => &self.key_search,
            Field::NewKey => &self.new_key,
            Field::Users => &self.users,
            Field::Increment => &self.increment,
            Field::OrderedStore => &self.ordered_store,
            Field::OrderedScope => &self.ordered_scope,
            Field::OrderedMin => &self.ordered_min,
            Field::OrderedMax => &self.ordered_max,
            Field::OrderedKey => &self.ordered_key,
            Field::OrderedValue => &self.ordered_value,
            Field::MapName => &self.map_name,
            Field::MapAfter => &self.map_after,
            Field::MapKey => &self.map_key,
            Field::MapTtl => &self.map_ttl,
            Field::MapSortKey => &self.map_sort_key,
            Field::QueueName => &self.queue_name,
            Field::QueueCount => &self.queue_count,
            Field::QueueInvisibility => &self.queue_invisibility,
            Field::QueuePriority => &self.queue_priority,
            Field::QueueTtl => &self.queue_ttl,
            Field::Value | Field::Attributes | Field::MapValue | Field::QueueData => return,
        };
        input.update(cx, |s, cx| s.set_value(text, window, cx));
    }

    /// `field`'s current text.
    pub(super) fn get(&self, field: Field, cx: &App) -> String {
        let value = match field {
            Field::Value => self.value.read(cx).value(),
            Field::Attributes => self.attributes.read(cx).value(),
            Field::MapValue => self.map_value.read(cx).value(),
            Field::QueueData => self.queue_data.read(cx).value(),
            Field::StoreSearch => self.store_search.read(cx).value(),
            Field::Scope => self.scope.read(cx).value(),
            Field::KeySearch => self.key_search.read(cx).value(),
            Field::NewKey => self.new_key.read(cx).value(),
            Field::Users => self.users.read(cx).value(),
            Field::Increment => self.increment.read(cx).value(),
            Field::OrderedStore => self.ordered_store.read(cx).value(),
            Field::OrderedScope => self.ordered_scope.read(cx).value(),
            Field::OrderedMin => self.ordered_min.read(cx).value(),
            Field::OrderedMax => self.ordered_max.read(cx).value(),
            Field::OrderedKey => self.ordered_key.read(cx).value(),
            Field::OrderedValue => self.ordered_value.read(cx).value(),
            Field::MapName => self.map_name.read(cx).value(),
            Field::MapAfter => self.map_after.read(cx).value(),
            Field::MapKey => self.map_key.read(cx).value(),
            Field::MapTtl => self.map_ttl.read(cx).value(),
            Field::MapSortKey => self.map_sort_key.read(cx).value(),
            Field::QueueName => self.queue_name.read(cx).value(),
            Field::QueueCount => self.queue_count.read(cx).value(),
            Field::QueueInvisibility => self.queue_invisibility.read(cx).value(),
            Field::QueuePriority => self.queue_priority.read(cx).value(),
            Field::QueueTtl => self.queue_ttl.read(cx).value(),
        };
        value.to_string()
    }

    pub(super) fn new(
        window: &mut Window,
        subscriptions: &mut Vec<Subscription>,
        cx: &mut Context<DataStoresWindow>,
    ) -> Self {
        let mut line = |field: Field, placeholder: &'static str, value: &'static str| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            });
            subscriptions.push(
                cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
                    this.on_input(field, event, cx)
                }),
            );
            input
        };
        let store_search = line(Field::StoreSearch, "Search data stores", "");
        let scope = line(Field::Scope, "global", "global");
        let key_search = line(Field::KeySearch, "Key prefix", "");
        let new_key = line(Field::NewKey, "New key name", "");
        let users = line(Field::Users, "UserIds, comma separated", "");
        let increment = line(Field::Increment, "1", "1");
        let ordered_store = line(Field::OrderedStore, "Ordered data store name", "");
        let ordered_scope = line(Field::OrderedScope, "global", "global");
        let ordered_min = line(Field::OrderedMin, "Min", "");
        let ordered_max = line(Field::OrderedMax, "Max", "");
        let ordered_key = line(Field::OrderedKey, "Key", "");
        let ordered_value = line(Field::OrderedValue, "Whole number", "");
        let map_name = line(Field::MapName, "Sorted map name", "");
        let map_after = line(Field::MapAfter, "Keys after\u{2026}", "");
        let map_key = line(Field::MapKey, "Key", "");
        let map_ttl = line(Field::MapTtl, "Seconds", "3600");
        let map_sort_key = line(Field::MapSortKey, "Sort key (optional)", "");
        let queue_name = line(Field::QueueName, "Queue name", "");
        let queue_count = line(Field::QueueCount, "10", "10");
        let queue_invisibility = line(Field::QueueInvisibility, "30", "30");
        let queue_priority = line(Field::QueuePriority, "0", "0");
        let queue_ttl = line(Field::QueueTtl, "Seconds", "3600");
        let mut area = |field: Field, rows: usize| {
            let input = cx.new(|cx| TextareaState::new(window, cx).rows(rows));
            subscriptions.push(
                cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
                    this.on_input(field, event, cx)
                }),
            );
            input
        };
        Inputs {
            store_search,
            scope,
            key_search,
            new_key,
            value: area(Field::Value, 14),
            users,
            attributes: area(Field::Attributes, 3),
            increment,
            ordered_store,
            ordered_scope,
            ordered_min,
            ordered_max,
            ordered_key,
            ordered_value,
            map_name,
            map_after,
            map_key,
            map_value: area(Field::MapValue, 8),
            map_ttl,
            map_sort_key,
            queue_name,
            queue_count,
            queue_invisibility,
            queue_data: area(Field::QueueData, 4),
            queue_priority,
            queue_ttl,
        }
    }
}
