//! Ordered data stores and the memory store (sorted maps and queues):
//! opening them by name, paging their contents, and the writes, which are
//! asked first like every other write.

use gpui_kit::*;

use rbx_cloud::{
    MapItemWrite, Page, SortKey, MAX_MEMORY_KEY_CHARS, MAX_MEMORY_TTL_SECONDS,
    MAX_MEMORY_VALUE_BYTES, MAX_NAME_CHARS, MAX_ORDERED_KEY_CHARS,
};
use serde_json::Value;

use super::state::{store_ref, ListId, Listing, Load, MemoryKind, Pending, Problem, Status};
use super::{fetch, json_tree, DataStoresWindow, Field};

const MAX_RECENT: usize = 8;
const MAX_SORT_KEY_CHARS: usize = 128;

fn remember(recent: &mut Vec<String>, name: &str) {
    recent.retain(|n| n != name);
    recent.insert(0, name.to_string());
    recent.truncate(MAX_RECENT);
}

fn fill<T>(list: &mut Listing<T>, reply: Result<Page<T>, Problem>, more: bool) -> bool {
    match reply {
        Ok(page) => {
            if more {
                list.items.extend(page.items);
            } else {
                list.items = page.items;
                list.selected = None;
            }
            list.next = page.next_page_token;
            list.load = Load::Idle;
            true
        }
        Err(problem) => {
            list.load = Load::Failed(problem);
            false
        }
    }
}

fn whole_number(text: &str, label: &str) -> Result<Option<i64>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<i64>()
        .map(Some)
        .map_err(|_| format!("{label} must be a whole number."))
}

fn sort_key(text: &str) -> Result<Option<SortKey>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if let Ok(number) = text.parse::<f64>() {
        if number.is_finite() {
            return Ok(Some(SortKey::Number(number)));
        }
    }
    if text.chars().count() > MAX_SORT_KEY_CHARS {
        return Err(format!(
            "The sort key can be at most {MAX_SORT_KEY_CHARS} characters."
        ));
    }
    Ok(Some(SortKey::Text(text.to_string())))
}

fn seconds(text: &str, label: &str, max: u64) -> Result<u64, String> {
    match text.trim().parse::<u64>() {
        Ok(n) if (1..=max).contains(&n) => Ok(n),
        _ => Err(format!("{label} must be a whole number from 1 to {max}.")),
    }
}

fn json_within_limit(text: &str) -> Result<Value, String> {
    let value = json_tree::parse_json(text)?;
    if rbx_cloud::value_bytes(&value) > MAX_MEMORY_VALUE_BYTES {
        return Err(format!(
            "The value can be at most {} KB.",
            MAX_MEMORY_VALUE_BYTES / 1024
        ));
    }
    Ok(value)
}

impl DataStoresWindow {
    fn more_request<T: Send + 'static>(
        &mut self,
        job: impl FnOnce() -> Result<T, Problem> + Send + 'static,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<T, Problem>, &mut Context<Self>) + 'static,
    ) {
        self.track_request(cx);
        cx.spawn(async move |this, cx| {
            let reply = cx.background_spawn(async move { job() }).await;
            let _ = this.update(cx, move |this, cx| {
                this.finish_request(cx);
                done(this, reply, cx);
            });
        })
        .detach();
    }

    fn more_say(&mut self, ok: bool, text: String) {
        self.status = Some(Status { ok, text });
    }

    /// A write: counts as in flight, reports `success` or the problem, and
    /// runs `after` on success.
    fn more_write<T: Send + 'static>(
        &mut self,
        success: String,
        job: impl FnOnce() -> Result<T, Problem> + Send + 'static,
        cx: &mut Context<Self>,
        after: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        self.writing += 1;
        self.more_request(job, cx, move |this, reply, cx| {
            this.writing = this.writing.saturating_sub(1);
            match reply {
                Ok(value) => {
                    this.more_say(true, success);
                    after(this, value, cx);
                }
                Err(problem) => this.more_say(false, fetch::describe(&problem)),
            }
            cx.notify();
        });
    }

    fn universe_id(&self) -> Option<u64> {
        self.universe.as_ref().map(|u| u.id)
    }

    // Ordered data stores.

    pub(super) fn open_ordered(&mut self, cx: &mut Context<Self>) {
        let name = self.inputs.get(Field::OrderedStore, cx).trim().to_string();
        self.open_ordered_with(name, cx);
    }

    pub(super) fn open_ordered_named(&mut self, name: String, cx: &mut Context<Self>) {
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

    pub(super) fn load_ordered(&mut self, more: bool, cx: &mut Context<Self>) {
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

    pub(super) fn set_ordered_descending(&mut self, on: bool, cx: &mut Context<Self>) {
        self.ordered.query.descending = on;
        if self.ordered.at.is_some() {
            self.load_ordered(false, cx);
        }
        cx.notify();
    }

    pub(super) fn select_ordered(&mut self, index: usize, cx: &mut Context<Self>) {
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

    pub(super) fn start_ordered_create(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn submit_ordered_edit(&mut self, creating: bool, cx: &mut Context<Self>) {
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

    pub(super) fn ask_ordered_increment(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn ask_ordered_delete(&mut self, cx: &mut Context<Self>) {
        if let (Some(at), Some(key)) = (self.ordered.at.clone(), self.selected_ordered_key()) {
            self.ask(Pending::DeleteOrdered { at, key }, cx);
        }
    }

    // Sorted maps.

    pub(super) fn open_map(&mut self, cx: &mut Context<Self>) {
        let name = self.inputs.get(Field::MapName, cx).trim().to_string();
        self.open_map_with(name, cx);
    }

    pub(super) fn open_map_named(&mut self, name: String, cx: &mut Context<Self>) {
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

    pub(super) fn load_map(&mut self, more: bool, cx: &mut Context<Self>) {
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

    pub(super) fn set_map_descending(&mut self, on: bool, cx: &mut Context<Self>) {
        self.map.descending = on;
        if self.map.name.is_some() {
            self.load_map(false, cx);
        }
        cx.notify();
    }

    pub(super) fn select_map_item(&mut self, index: usize, cx: &mut Context<Self>) {
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

    pub(super) fn start_map_edit(&mut self, creating: bool, cx: &mut Context<Self>) {
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

    pub(super) fn submit_map_edit(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn ask_map_delete(&mut self, cx: &mut Context<Self>) {
        let (Some(map), Some(item)) = (self.map.name.clone(), self.selected_map_item()) else {
            return;
        };
        let key = item.key.clone();
        self.ask(Pending::DeleteMapItem { map, key }, cx);
    }

    // Queues.

    pub(super) fn open_queue(&mut self, cx: &mut Context<Self>) {
        let name = self.inputs.get(Field::QueueName, cx).trim().to_string();
        self.open_queue_with(name, cx);
    }

    pub(super) fn open_queue_named(&mut self, name: String, cx: &mut Context<Self>) {
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

    pub(super) fn read_queue(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn start_queue_add(&mut self, on: bool, cx: &mut Context<Self>) {
        if on {
            self.fill(Field::QueueData, "{}");
        }
        self.queue.adding = on && self.queue.name.is_some();
        self.queue.error = None;
        cx.notify();
    }

    pub(super) fn submit_queue_add(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn ask_queue_discard(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn set_memory_kind(&mut self, kind: MemoryKind, cx: &mut Context<Self>) {
        self.memory_kind = kind;
        self.active_list = match kind {
            MemoryKind::SortedMap => ListId::MapItems,
            MemoryKind::Queue => ListId::QueueItems,
        };
        cx.notify();
    }

    /// Runs the ordered and memory writes; any other pending is handed back.
    pub(super) fn run_pending_more(
        &mut self,
        pending: Pending,
        cx: &mut Context<Self>,
    ) -> Option<Pending> {
        let Some(universe) = self.universe_id() else {
            return match pending {
                Pending::SaveOrdered { .. }
                | Pending::IncrementOrdered { .. }
                | Pending::DeleteOrdered { .. }
                | Pending::SaveMapItem { .. }
                | Pending::DeleteMapItem { .. }
                | Pending::AddQueueItem { .. }
                | Pending::ReadQueue { .. }
                | Pending::DiscardQueue { .. } => None,
                other => Some(other),
            };
        };
        match pending {
            Pending::SaveOrdered {
                at,
                key,
                value,
                creating,
            } => {
                let verb = if creating { "Created" } else { "Saved" };
                let done = format!("{verb} \u{201c}{key}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        let client = fetch::client()?;
                        if creating {
                            client.create_ordered(&at, &key, value)
                        } else {
                            client.update_ordered(&at, &key, value)
                        }
                        .map(|_| ())
                        .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| this.load_ordered(false, cx),
                );
            }
            Pending::IncrementOrdered { at, key, amount } => {
                let done = format!("Incremented \u{201c}{key}\u{201d} by {amount}.");
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .increment_ordered(&at, &key, amount)
                            .map(|_| ())
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| this.load_ordered(false, cx),
                );
            }
            Pending::DeleteOrdered { at, key } => {
                let done = format!("Deleted \u{201c}{key}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .delete_ordered(&at, &key)
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| {
                        this.fill(Field::OrderedKey, "");
                        this.fill(Field::OrderedValue, "");
                        this.load_ordered(false, cx);
                    },
                );
            }
            Pending::SaveMapItem {
                map,
                key,
                write,
                creating,
            } => {
                let verb = if creating { "Created" } else { "Saved" };
                let done = format!("{verb} \u{201c}{key}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        let client = fetch::client()?;
                        if creating {
                            client.create_map_item(universe, &map, &key, &write)
                        } else {
                            client.update_map_item(universe, &map, &key, &write)
                        }
                        .map(|_| ())
                        .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| {
                        this.map.creating = None;
                        this.load_map(false, cx);
                    },
                );
            }
            Pending::DeleteMapItem { map, key } => {
                let done = format!("Deleted \u{201c}{key}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .delete_map_item(universe, &map, &key)
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| this.load_map(false, cx),
                );
            }
            Pending::AddQueueItem {
                queue,
                data,
                priority,
                ttl_seconds,
            } => {
                let done = format!("Added an item to \u{201c}{queue}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .add_queue_item(universe, &queue, &data, priority, ttl_seconds)
                            .map(|_| ())
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| {
                        this.queue.adding = false;
                        cx.notify();
                    },
                );
            }
            Pending::ReadQueue {
                queue,
                count,
                invisibility_seconds,
            } => {
                self.more_write(
                    "Read the queue.".to_string(),
                    move || {
                        fetch::client()?
                            .read_queue(universe, &queue, count, invisibility_seconds)
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, read, cx| {
                        let n = read.items.len();
                        let text = if n == 1 {
                            "Read 1 item.".to_string()
                        } else {
                            format!("Read {n} items.")
                        };
                        this.more_say(true, text);
                        this.queue.read = Some(read);
                        this.debug_step(cx);
                    },
                );
            }
            Pending::DiscardQueue {
                queue,
                read_id,
                count,
            } => {
                let done = if count == 1 {
                    "Discarded 1 item.".to_string()
                } else {
                    format!("Discarded {count} items.")
                };
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .discard_queue_items(universe, &queue, &read_id)
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| {
                        this.queue.read = None;
                        cx.notify();
                    },
                );
            }
            other => return Some(other),
        }
        None
    }
}
