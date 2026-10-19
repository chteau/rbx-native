//! The memory store: sorted maps and queues.

use gpui_kit::*;

use rbx_cloud::{MapItemWrite, SortKey, MAX_MEMORY_KEY_CHARS, MAX_MEMORY_TTL_SECONDS};

use super::super::state::{ListId, Listing, Load, MemoryKind, Pending};
use super::super::{fetch, json_tree, DataStoresWindow, Field};
use super::{fill, json_within_limit, remember, seconds, sort_key};

impl DataStoresWindow {
    // Sorted maps.

    pub(in super::super) fn open_map(&mut self, cx: &mut Context<Self>) {
        let name = self.inputs.get(Field::MapName, cx).trim().to_string();
        self.open_map_with(name, cx);
    }

    pub(in super::super) fn open_map_named(&mut self, name: String, cx: &mut Context<Self>) {
        self.fill(Field::MapName, name.clone());
        self.open_map_with(name, cx);
    }

    fn open_map_with(&mut self, name: String, cx: &mut Context<Self>) {
        if self.universe.is_none() {
            return;
        }
        if name.is_empty() {
            self.map.error = Some("Enter the name of a sorted map.".to_string());
            cx.notify();
            return;
        }
        remember(&mut self.map.recent, &name);
        self.map.name = Some(name);
        self.map.error = None;
        self.map.creating = None;
        self.map.collapsed.clear();
        self.map.list = Listing {
            generation: self.map.list.generation + 1,
            ..Listing::default()
        };
        self.active_list = ListId::MapItems;
        self.load_map(false, cx);
    }

    pub(in super::super) fn load_map(&mut self, more: bool, cx: &mut Context<Self>) {
        let (Some(universe), Some(name)) = (self.universe_id(), self.map.name.clone()) else {
            return;
        };
        let token = if more {
            match self.map.list.next.clone() {
                Some(token) if self.map.list.load != Load::Busy => Some(token),
                _ => return,
            }
        } else {
            self.map.list.generation += 1;
            None
        };
        let generation = self.map.list.generation;
        self.map.list.load = Load::Busy;
        cx.notify();
        let after = self.inputs.get(Field::MapAfter, cx).trim().to_string();
        let descending = self.map.descending;
        self.more_request(
            move || {
                let after = (!after.is_empty()).then_some(after);
                fetch::client()?
                    .list_map_items(
                        universe,
                        &name,
                        descending,
                        after.as_deref(),
                        token.as_deref(),
                    )
                    .map_err(|e| fetch::problem(&e))
            },
            cx,
            move |this, reply, cx| {
                if generation != this.map.list.generation {
                    return;
                }
                if fill(&mut this.map.list, reply, more) {
                    this.debug_step(cx);
                }
                cx.notify();
            },
        );
    }

    pub(in super::super) fn set_map_descending(&mut self, on: bool, cx: &mut Context<Self>) {
        self.map.descending = on;
        if self.map.name.is_some() {
            self.load_map(false, cx);
        }
        cx.notify();
    }

    pub(in super::super) fn select_map_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(item) = self.map.list.items.get(index) else {
            return;
        };
        let key = item.key.clone();
        let value = json_tree::pretty(&item.value);
        let sort = match &item.sort_key {
            Some(SortKey::Number(n)) => n.to_string(),
            Some(SortKey::Text(text)) => text.clone(),
            None => String::new(),
        };
        self.map.list.selected = Some(index);
        self.map.creating = None;
        self.map.error = None;
        self.map.collapsed.clear();
        self.fill(Field::MapKey, key);
        self.fill(Field::MapValue, value);
        self.fill(Field::MapSortKey, sort);
        self.fill(Field::MapTtl, "3600");
        cx.notify();
    }

    fn selected_map_item(&self) -> Option<&rbx_cloud::MapItem> {
        self.map.list.items.get(self.map.list.selected?)
    }

    pub(in super::super) fn start_map_edit(&mut self, creating: bool, cx: &mut Context<Self>) {
        if self.map.name.is_none() || (!creating && self.selected_map_item().is_none()) {
            return;
        }
        if creating {
            self.map.list.selected = None;
            self.fill(Field::MapKey, "");
            self.fill(Field::MapValue, "{}");
            self.fill(Field::MapSortKey, "");
        }
        self.fill(Field::MapTtl, "3600");
        self.map.error = None;
        self.map.creating = Some(creating);
        cx.notify();
    }

    fn map_fail(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.map.error = Some(message.into());
        cx.notify();
    }

    pub(in super::super) fn submit_map_edit(&mut self, cx: &mut Context<Self>) {
        let (Some(creating), Some(map)) = (self.map.creating, self.map.name.clone()) else {
            return;
        };
        let (key, etag) = if creating {
            (self.inputs.get(Field::MapKey, cx).trim().to_string(), None)
        } else {
            match self.selected_map_item() {
                Some(item) => (item.key.clone(), Some(item.etag.clone())),
                None => return,
            }
        };
        let chars = key.chars().count();
        if chars == 0 || chars > MAX_MEMORY_KEY_CHARS {
            self.map_fail(
                format!("The key must be 1 to {MAX_MEMORY_KEY_CHARS} characters."),
                cx,
            );
            return;
        }
        let value = match json_within_limit(&self.inputs.get(Field::MapValue, cx)) {
            Ok(value) => value,
            Err(error) => return self.map_fail(error, cx),
        };
        let ttl = seconds(
            &self.inputs.get(Field::MapTtl, cx),
            "Expires after",
            MAX_MEMORY_TTL_SECONDS,
        );
        let ttl_seconds = match ttl {
            Ok(ttl) => ttl,
            Err(error) => return self.map_fail(error, cx),
        };
        let sort_key = match sort_key(&self.inputs.get(Field::MapSortKey, cx)) {
            Ok(sort_key) => sort_key,
            Err(error) => return self.map_fail(error, cx),
        };
        self.map.error = None;
        let write = MapItemWrite {
            value,
            ttl_seconds,
            sort_key,
            etag,
        };
        self.ask(
            Pending::SaveMapItem {
                map,
                key,
                write,
                creating,
            },
            cx,
        );
    }

    pub(in super::super) fn ask_map_delete(&mut self, cx: &mut Context<Self>) {
        let (Some(map), Some(item)) = (self.map.name.clone(), self.selected_map_item()) else {
            return;
        };
        let key = item.key.clone();
        self.ask(Pending::DeleteMapItem { map, key }, cx);
    }

    // Queues.

    pub(in super::super) fn open_queue(&mut self, cx: &mut Context<Self>) {
        let name = self.inputs.get(Field::QueueName, cx).trim().to_string();
        self.open_queue_with(name, cx);
    }

    pub(in super::super) fn open_queue_named(&mut self, name: String, cx: &mut Context<Self>) {
        self.fill(Field::QueueName, name.clone());
        self.open_queue_with(name, cx);
    }

    fn open_queue_with(&mut self, name: String, cx: &mut Context<Self>) {
        if self.universe.is_none() {
            return;
        }
        if name.is_empty() {
            self.queue.error = Some("Enter the name of a queue.".to_string());
            cx.notify();
            return;
        }
        remember(&mut self.queue.recent, &name);
        self.queue.name = Some(name);
        self.queue.read = None;
        self.queue.adding = false;
        self.queue.error = None;
        self.queue.generation += 1;
        self.active_list = ListId::QueueItems;
        cx.notify();
    }

    fn queue_fail(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.queue.error = Some(message.into());
        cx.notify();
    }

    pub(in super::super) fn read_queue(&mut self, cx: &mut Context<Self>) {
        let Some(queue) = self.queue.name.clone() else {
            return;
        };
        let count = self.inputs.get(Field::QueueCount, cx);
        let count = match seconds(&count, "Items", 200) {
            Ok(n) => n as u32,
            Err(error) => return self.queue_fail(error, cx),
        };
        let window = self.inputs.get(Field::QueueInvisibility, cx);
        let invisibility_seconds = match seconds(&window, "Invisible for", 43_200) {
            Ok(n) => n as u32,
            Err(error) => return self.queue_fail(error, cx),
        };
        self.queue.error = None;
        let pending = Pending::ReadQueue {
            queue,
            count,
            invisibility_seconds,
        };
        if self.universe.as_ref().is_some_and(|u| u.live) {
            self.ask(pending, cx);
        } else {
            self.run_pending_more(pending, cx);
        }
    }

    pub(in super::super) fn start_queue_add(&mut self, on: bool, cx: &mut Context<Self>) {
        if on {
            self.fill(Field::QueueData, "{}");
        }
        self.queue.adding = on && self.queue.name.is_some();
        self.queue.error = None;
        cx.notify();
    }

    pub(in super::super) fn submit_queue_add(&mut self, cx: &mut Context<Self>) {
        let Some(queue) = self.queue.name.clone() else {
            return;
        };
        let data = match json_within_limit(&self.inputs.get(Field::QueueData, cx)) {
            Ok(data) => data,
            Err(error) => return self.queue_fail(error, cx),
        };
        let priority = match self
            .inputs
            .get(Field::QueuePriority, cx)
            .trim()
            .parse::<f64>()
        {
            Ok(p) if p.is_finite() => p,
            _ => return self.queue_fail("Priority must be a number.", cx),
        };
        let ttl = seconds(
            &self.inputs.get(Field::QueueTtl, cx),
            "Expires after",
            MAX_MEMORY_TTL_SECONDS,
        );
        let ttl_seconds = match ttl {
            Ok(ttl) => ttl,
            Err(error) => return self.queue_fail(error, cx),
        };
        self.queue.error = None;
        self.ask(
            Pending::AddQueueItem {
                queue,
                data,
                priority,
                ttl_seconds,
            },
            cx,
        );
    }

    pub(in super::super) fn ask_queue_discard(&mut self, cx: &mut Context<Self>) {
        let Some(queue) = self.queue.name.clone() else {
            return;
        };
        let Some(read) = &self.queue.read else {
            return;
        };
        if let Some(read_id) = read.read_id.clone().filter(|_| !read.items.is_empty()) {
            let count = read.items.len();
            self.ask(
                Pending::DiscardQueue {
                    queue,
                    read_id,
                    count,
                },
                cx,
            );
        }
    }

    pub(in super::super) fn set_memory_kind(&mut self, kind: MemoryKind, cx: &mut Context<Self>) {
        self.memory_kind = kind;
        self.active_list = match kind {
            MemoryKind::SortedMap => ListId::MapItems,
            MemoryKind::Queue => ListId::QueueItems,
        };
        cx.notify();
    }
}
