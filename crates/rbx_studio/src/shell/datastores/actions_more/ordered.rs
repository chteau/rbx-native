//! Ordered data stores: opening by name, paging, and the edits.

use gpui_kit::*;

use rbx_cloud::{MAX_NAME_CHARS, MAX_ORDERED_KEY_CHARS};

use super::super::state::{store_ref, ListId, Listing, Load, Pending};
use super::super::{fetch, DataStoresWindow, Field};
use super::{fill, remember, whole_number};

impl DataStoresWindow {
    pub(in super::super) fn open_ordered(&mut self, cx: &mut Context<Self>) {
        let name = self.inputs.get(Field::OrderedStore, cx).trim().to_string();
        self.open_ordered_with(name, cx);
    }

    pub(in super::super) fn open_ordered_named(&mut self, name: String, cx: &mut Context<Self>) {
        self.fill(Field::OrderedStore, name.clone());
        self.open_ordered_with(name, cx);
    }

    fn open_ordered_with(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(universe) = self.universe_id() else {
            return;
        };
        let chars = name.chars().count();
        if chars == 0 || chars > MAX_NAME_CHARS {
            self.ordered.error = Some(format!(
                "The store name must be 1 to {MAX_NAME_CHARS} characters."
            ));
            cx.notify();
            return;
        }
        let scope = self.inputs.get(Field::OrderedScope, cx);
        let scope = scope.trim();
        let scope = if scope.is_empty() { "global" } else { scope };
        self.ordered.at = Some(store_ref(universe, &name, scope));
        remember(&mut self.ordered.recent, &name);
        self.ordered.error = None;
        self.ordered.list = Listing {
            generation: self.ordered.list.generation + 1,
            ..Listing::default()
        };
        self.fill(Field::OrderedKey, "");
        self.fill(Field::OrderedValue, "");
        self.active_list = ListId::Ordered;
        self.load_ordered(false, cx);
    }

    pub(in super::super) fn load_ordered(&mut self, more: bool, cx: &mut Context<Self>) {
        let Some(at) = self.ordered.at.clone() else {
            return;
        };
        let min = whole_number(&self.inputs.get(Field::OrderedMin, cx), "Min");
        let max = whole_number(&self.inputs.get(Field::OrderedMax, cx), "Max");
        let (min, max) = match (min, max) {
            (Ok(min), Ok(max)) => (min, max),
            (Err(error), _) | (_, Err(error)) => {
                self.ordered.error = Some(error);
                cx.notify();
                return;
            }
        };
        self.ordered.error = None;
        self.ordered.query.min = min;
        self.ordered.query.max = max;
        let token = if more {
            match self.ordered.list.next.clone() {
                Some(token) if self.ordered.list.load != Load::Busy => Some(token),
                _ => return,
            }
        } else {
            self.ordered.list.generation += 1;
            None
        };
        let generation = self.ordered.list.generation;
        self.ordered.list.load = Load::Busy;
        cx.notify();
        let query = self.ordered.query;
        self.more_request(
            move || {
                fetch::client()?
                    .list_ordered(&at, query, token.as_deref())
                    .map_err(|e| fetch::problem(&e))
            },
            cx,
            move |this, reply, cx| {
                if generation != this.ordered.list.generation {
                    return;
                }
                if fill(&mut this.ordered.list, reply, more) {
                    this.debug_step(cx);
                }
                cx.notify();
            },
        );
    }

    pub(in super::super) fn set_ordered_descending(&mut self, on: bool, cx: &mut Context<Self>) {
        self.ordered.query.descending = on;
        if self.ordered.at.is_some() {
            self.load_ordered(false, cx);
        }
        cx.notify();
    }

    pub(in super::super) fn select_ordered(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.ordered.list.items.get(index) else {
            return;
        };
        let (key, value) = (entry.key.clone(), entry.value.to_string());
        self.ordered.list.selected = Some(index);
        self.ordered.error = None;
        self.fill(Field::OrderedKey, key);
        self.fill(Field::OrderedValue, value);
        cx.notify();
    }

    pub(in super::super) fn start_ordered_create(&mut self, cx: &mut Context<Self>) {
        self.ordered.list.selected = None;
        self.ordered.error = None;
        self.fill(Field::OrderedKey, "");
        self.fill(Field::OrderedValue, "");
        cx.notify();
    }

    fn selected_ordered_key(&self) -> Option<String> {
        let index = self.ordered.list.selected?;
        self.ordered.list.items.get(index).map(|e| e.key.clone())
    }

    fn ordered_fail(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.ordered.error = Some(message.into());
        cx.notify();
    }

    pub(in super::super) fn submit_ordered_edit(&mut self, creating: bool, cx: &mut Context<Self>) {
        let Some(at) = self.ordered.at.clone() else {
            return;
        };
        let key = if creating {
            self.inputs.get(Field::OrderedKey, cx).trim().to_string()
        } else {
            match self.selected_ordered_key() {
                Some(key) => key,
                None => return,
            }
        };
        let chars = key.chars().count();
        if chars == 0 || chars > MAX_ORDERED_KEY_CHARS {
            self.ordered_fail(
                format!("The key must be 1 to {MAX_ORDERED_KEY_CHARS} characters."),
                cx,
            );
            return;
        }
        let Ok(Some(value)) = whole_number(&self.inputs.get(Field::OrderedValue, cx), "Value")
        else {
            self.ordered_fail("Value must be a whole number.", cx);
            return;
        };
        self.ordered.error = None;
        self.ask(
            Pending::SaveOrdered {
                at,
                key,
                value,
                creating,
            },
            cx,
        );
    }

    pub(in super::super) fn ask_ordered_increment(&mut self, cx: &mut Context<Self>) {
        let (Some(at), Some(key)) = (self.ordered.at.clone(), self.selected_ordered_key()) else {
            return;
        };
        match whole_number(&self.inputs.get(Field::Increment, cx), "Amount") {
            Ok(Some(amount)) if amount != 0 => {
                self.ordered.error = None;
                self.ask(Pending::IncrementOrdered { at, key, amount }, cx);
            }
            _ => self.ordered_fail("Amount must be a non-zero whole number.", cx),
        }
    }

    pub(in super::super) fn ask_ordered_delete(&mut self, cx: &mut Context<Self>) {
        if let (Some(at), Some(key)) = (self.ordered.at.clone(), self.selected_ordered_key()) {
            self.ask(Pending::DeleteOrdered { at, key }, cx);
        }
    }
}
