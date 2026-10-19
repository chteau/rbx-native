//! What the window does in answer to clicks, keys and typing: loading the
//! lists, opening an entry, editing, and the experience in use. The writes
//! themselves are in `writes`.

use std::time::Duration;

use gpui_kit::component::input::InputEvent;
use gpui_kit::*;

use rbx_cloud::{Page, StoreRef, ALL_SCOPES};
use serde_json::Value;

use super::state::{
    store_ref, EntryDraft, EntryPane, EntryTab, ListId, Listing, Load, MemoryKind, Problem,
    Section, Status, TreeTarget, Universe, UniverseSource, ValueView,
};
use super::{fetch, json_tree, DataStoresWindow, Field};

mod writes;

/// Waiting this long after the last keystroke keeps a search from firing a
/// request per letter.
const TYPING_PAUSE: Duration = Duration::from_millis(250);
const POLL_EVERY: Duration = Duration::from_millis(300);

/// Stores `reply` in `list`; `true` when it was a page.
fn fill_page<T>(list: &mut Listing<T>, reply: Result<Page<T>, Problem>, more: bool) -> bool {
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

impl DataStoresWindow {
    /// Runs `job` off the UI thread, then `done` with its result.
    fn request<T: Send + 'static>(
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

    fn say(&mut self, ok: bool, text: String) {
        self.status = Some(Status { ok, text });
    }

    /// The scope field as the API wants it.
    fn scope_value(&self, cx: &App) -> String {
        let text = self.inputs.get(Field::Scope, cx);
        let text = text.trim();
        match text.to_lowercase().as_str() {
            "" => "global".to_string(),
            "-" | "*" | "all" => ALL_SCOPES.to_string(),
            _ => text.to_string(),
        }
    }

    /// Store, key and etag of the entry in the pane.
    fn entry_target(&self) -> Option<(StoreRef, String, String)> {
        let universe = self.universe.as_ref()?.id;
        let store = self.store.as_ref()?;
        let entry = self.pane.entry.as_ref()?;
        Some((
            store_ref(universe, store, &entry.scope),
            entry.key.clone(),
            entry.etag.clone(),
        ))
    }

    /// Runs `action` once the field still holds what it does now.
    fn after_pause(
        &mut self,
        field: Field,
        cx: &mut Context<Self>,
        action: fn(&mut Self, &mut Context<Self>),
    ) {
        let text = self.inputs.get(field, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_PAUSE).await;
            let _ = this.update(cx, |this, cx| {
                if this.inputs.get(field, cx) == text {
                    action(this, cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn on_input(&mut self, field: Field, event: &InputEvent, cx: &mut Context<Self>) {
        let change = matches!(event, InputEvent::Change);
        let enter = matches!(event, InputEvent::PressEnter { .. });
        let blur = matches!(event, InputEvent::Blur);
        match field {
            Field::StoreSearch if change => self.after_pause(field, cx, Self::load_stores),
            Field::KeySearch if change => self.after_pause(field, cx, Self::load_keys),
            Field::Scope if enter || blur => self.load_keys(cx),
            Field::Value | Field::Users | Field::Attributes | Field::NewKey if change => {
                if let Some(draft) = &mut self.pane.editing {
                    if draft.error.take().is_some() {
                        cx.notify();
                    }
                }
            }
            Field::OrderedStore | Field::OrderedScope if enter => self.open_ordered(cx),
            Field::OrderedMin | Field::OrderedMax if enter => self.load_ordered(false, cx),
            Field::MapName if enter => self.open_map(cx),
            Field::MapAfter if enter => self.load_map(false, cx),
            Field::QueueName if enter => self.open_queue(cx),
            _ => {}
        }
    }

    pub(super) fn load_stores(&mut self, cx: &mut Context<Self>) {
        self.fetch_stores(false, cx);
    }

    pub(super) fn load_more_stores(&mut self, cx: &mut Context<Self>) {
        self.fetch_stores(true, cx);
    }

    fn fetch_stores(&mut self, more: bool, cx: &mut Context<Self>) {
        let Some(universe) = self.universe.as_ref().map(|u| u.id) else {
            return;
        };
        let token = if more {
            match self.stores.next.clone() {
                Some(token) if self.stores.load != Load::Busy => Some(token),
                _ => return,
            }
        } else {
            self.stores.generation += 1;
            None
        };
        let generation = self.stores.generation;
        self.stores.load = Load::Busy;
        cx.notify();
        let prefix = self.inputs.get(Field::StoreSearch, cx);
        let deleted = self.show_deleted;
        self.request(
            move || {
                fetch::client()?
                    .list_data_stores(universe, &prefix, deleted, token.as_deref())
                    .map_err(|e| fetch::problem(&e))
            },
            cx,
            move |this, reply, cx| {
                if generation != this.stores.generation {
                    return;
                }
                if fill_page(&mut this.stores, reply, more) {
                    if !more {
                        let open = this.store.clone();
                        this.stores.selected =
                            open.and_then(|n| this.stores.items.iter().position(|s| s.id == n));
                    }
                    this.debug_step(cx);
                }
                cx.notify();
            },
        );
    }

    pub(super) fn select_store(&mut self, name: String, cx: &mut Context<Self>) {
        self.stores.selected = self.stores.items.iter().position(|s| s.id == name);
        self.store = Some(name);
        self.keys = Listing {
            generation: self.keys.generation + 1,
            ..Listing::default()
        };
        self.pane = EntryPane {
            generation: self.pane.generation + 1,
            view: self.pane.view,
            ..EntryPane::default()
        };
        self.load_keys(cx);
    }

    pub(super) fn load_keys(&mut self, cx: &mut Context<Self>) {
        self.fetch_keys(false, cx);
    }

    pub(super) fn load_more_keys(&mut self, cx: &mut Context<Self>) {
        self.fetch_keys(true, cx);
    }

    fn fetch_keys(&mut self, more: bool, cx: &mut Context<Self>) {
        let (Some(universe), Some(store)) =
            (self.universe.as_ref().map(|u| u.id), self.store.clone())
        else {
            return;
        };
        let token = if more {
            match self.keys.next.clone() {
                Some(token) if self.keys.load != Load::Busy => Some(token),
                _ => return,
            }
        } else {
            self.keys.generation += 1;
            None
        };
        let generation = self.keys.generation;
        self.keys.load = Load::Busy;
        cx.notify();
        let at = store_ref(universe, &store, &self.scope_value(cx));
        let prefix = self.inputs.get(Field::KeySearch, cx).trim().to_string();
        self.request(
            move || {
                fetch::client()?
                    .list_entries(&at, &prefix, token.as_deref())
                    .map_err(|e| fetch::problem(&e))
            },
            cx,
            move |this, reply, cx| {
                if generation != this.keys.generation {
                    return;
                }
                if fill_page(&mut this.keys, reply, more) {
                    if !more {
                        let open = this
                            .pane
                            .entry
                            .as_ref()
                            .map(|e| (e.scope.clone(), e.key.clone()));
                        this.keys.selected = open.and_then(|(scope, key)| {
                            this.keys
                                .items
                                .iter()
                                .position(|k| k.scope == scope && k.key == key)
                        });
                    }
                    this.debug_step(cx);
                }
                cx.notify();
            },
        );
    }

    pub(super) fn select_key(&mut self, scope: String, key: String, cx: &mut Context<Self>) {
        let (Some(universe), Some(store)) =
            (self.universe.as_ref().map(|u| u.id), self.store.clone())
        else {
            return;
        };
        self.keys.selected = self
            .keys
            .items
            .iter()
            .position(|k| k.scope == scope && k.key == key);
        self.active_list = ListId::Keys;
        let generation = self.pane.generation + 1;
        self.pane = EntryPane {
            generation,
            tab: self.pane.tab,
            view: self.pane.view,
            load: Load::Busy,
            ..EntryPane::default()
        };
        cx.notify();
        let at = store_ref(universe, &store, &scope);
        self.request(
            move || {
                fetch::client()?
                    .get_entry(&at, &key)
                    .map_err(|e| fetch::problem(&e))
            },
            cx,
            move |this, reply, cx| {
                if generation != this.pane.generation {
                    return;
                }
                match reply {
                    Ok(entry) => {
                        this.pane.entry = Some(entry);
                        this.pane.load = Load::Idle;
                        if this.pane.tab == EntryTab::Versions {
                            this.load_revisions(false, cx);
                        }
                        this.debug_step(cx);
                    }
                    Err(problem) => this.pane.load = Load::Failed(problem),
                }
                cx.notify();
            },
        );
    }

    pub(super) fn load_revisions(&mut self, more: bool, cx: &mut Context<Self>) {
        let Some((at, key, _)) = self.entry_target() else {
            return;
        };
        let token = if more {
            match self.pane.revisions.next.clone() {
                Some(token) if self.pane.revisions.load != Load::Busy => Some(token),
                _ => return,
            }
        } else {
            self.pane.revisions.generation += 1;
            None
        };
        let generation = self.pane.revisions.generation;
        let pane = self.pane.generation;
        self.pane.revisions.load = Load::Busy;
        cx.notify();
        self.request(
            move || {
                fetch::client()?
                    .list_revisions(&at, &key, token.as_deref())
                    .map_err(|e| fetch::problem(&e))
            },
            cx,
            move |this, reply, cx| {
                if pane != this.pane.generation || generation != this.pane.revisions.generation {
                    return;
                }
                if fill_page(&mut this.pane.revisions, reply, more) {
                    this.debug_step(cx);
                }
                cx.notify();
            },
        );
    }

    pub(super) fn view_revision(&mut self, revision_id: String, cx: &mut Context<Self>) {
        let Some((at, key, _)) = self.entry_target() else {
            return;
        };
        let generation = self.pane.generation;
        self.request(
            move || {
                fetch::client()?
                    .get_entry_revision(&at, &key, &revision_id)
                    .map_err(|e| fetch::problem(&e))
            },
            cx,
            move |this, reply, cx| {
                if generation != this.pane.generation {
                    return;
                }
                match reply {
                    Ok(entry) => {
                        this.pane.revision = Some(entry);
                        this.pane.tab = EntryTab::Value;
                        this.debug_step(cx);
                    }
                    Err(problem) => this.say(false, fetch::describe(&problem)),
                }
                cx.notify();
            },
        );
    }

    pub(super) fn start_edit(&mut self, creating: bool, cx: &mut Context<Self>) {
        if creating {
            if self.store.is_none() {
                return;
            }
            self.fill(Field::Value, "{}");
            self.fill(Field::Users, "");
            self.fill(Field::Attributes, "{}");
        } else {
            let Some(entry) = &self.pane.entry else {
                return;
            };
            let users = entry
                .users
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let attributes = if entry.attributes.is_empty() {
                "{}".to_string()
            } else {
                json_tree::pretty(&Value::Object(entry.attributes.clone()))
            };
            let value = json_tree::pretty(&entry.value);
            self.fill(Field::Value, value);
            self.fill(Field::Users, users);
            self.fill(Field::Attributes, attributes);
        }
        self.fill(Field::NewKey, "");
        self.pane.tab = EntryTab::Value;
        self.pane.revision = None;
        self.pane.editing = Some(EntryDraft {
            creating,
            error: None,
        });
        cx.notify();
    }

    pub(super) fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.pane.editing = None;
        cx.notify();
    }

    pub(super) fn pick_experience(&mut self, cx: &mut Context<Self>) {
        let window = cx.entity().downgrade();
        // Deferred: not from inside this update, which may be a render's.
        cx.defer(move |cx| {
            let on_pick = {
                let window = window.clone();
                move |experience: rbx_cloud::Experience, cx: &mut App| {
                    let _ = window.update(cx, |this, cx| {
                        let live = !this.is_sandbox(experience.universe_id, cx);
                        this.set_universe(
                            Some(Universe {
                                id: experience.universe_id,
                                name: Some(experience.name),
                                source: UniverseSource::Picked,
                                live,
                            }),
                            cx,
                        );
                    });
                }
            };
            let opened = crate::launcher::open_game_picker(on_pick, cx);
            let _ = window.update(cx, |this, _| this.picker = opened);
        });
    }

    pub(super) fn use_sandbox(&mut self, cx: &mut Context<Self>) {
        let sandbox = self.shell.read(cx).play.sandbox.map(|s| s.universe_id);
        if let Some(id) = sandbox {
            self.set_universe(
                Some(Universe {
                    id,
                    name: None,
                    source: UniverseSource::Sandbox,
                    live: false,
                }),
                cx,
            );
        }
    }

    pub(super) fn manage_key(&mut self, cx: &mut Context<Self>) {
        cx.defer(|cx| {
            crate::launcher::open_publishing(|_cx| {}, cx);
        });
    }

    pub(super) fn set_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        self.active_list = match section {
            Section::Standard => ListId::Stores,
            Section::Ordered => ListId::Ordered,
            Section::Memory => match self.memory_kind {
                MemoryKind::SortedMap => ListId::MapItems,
                MemoryKind::Queue => ListId::QueueItems,
            },
        };
        cx.notify();
    }

    pub(super) fn track_request(&mut self, cx: &mut Context<Self>) {
        self.in_flight += 1;
        if self.in_flight == 1 {
            self.waits_seen = rbx_cloud::rate_limit_waits();
            self._poll = Some(cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(POLL_EVERY).await;
                let alive = this.update(cx, |this, cx| {
                    let limited = rbx_cloud::rate_limit_waits() > this.waits_seen;
                    if limited != this.rate_limited {
                        this.rate_limited = limited;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }));
        }
        cx.notify();
    }

    pub(super) fn finish_request(&mut self, cx: &mut Context<Self>) {
        self.in_flight = self.in_flight.saturating_sub(1);
        if self.in_flight == 0 {
            self._poll = None;
            self.rate_limited = false;
            self.waits_seen = 0;
        }
        cx.notify();
    }

    pub(super) fn copy(&mut self, text: String, what: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.say(true, format!("Copied {what}."));
        cx.notify();
    }

    pub(super) fn toggle_tree(&mut self, target: TreeTarget, path: String, cx: &mut Context<Self>) {
        let folded = match target {
            TreeTarget::Entry => &mut self.pane.collapsed,
            TreeTarget::MapItem => &mut self.map.collapsed,
        };
        if !folded.remove(&path) {
            folded.insert(path);
        }
        cx.notify();
    }

    fn list_extent(&self, list: ListId) -> (usize, Option<usize>) {
        match list {
            ListId::Stores => (self.stores.items.len(), self.stores.selected),
            ListId::Keys => (self.keys.items.len(), self.keys.selected),
            ListId::Revisions => (
                self.pane.revisions.items.len(),
                self.pane.revisions.selected,
            ),
            ListId::Ordered => (self.ordered.list.items.len(), self.ordered.list.selected),
            ListId::MapItems => (self.map.list.items.len(), self.map.list.selected),
            ListId::QueueItems => (0, None),
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let list = self.active_list;
        let (len, current) = self.list_extent(list);
        if len == 0 {
            return;
        }
        let last = len as isize - 1;
        let next = match current {
            Some(at) => (at as isize + delta).clamp(0, last),
            None if delta < 0 => last,
            None => 0,
        };
        self.select_row(list, next as usize, cx);
    }

    pub(super) fn activate_selection(&mut self, cx: &mut Context<Self>) {
        let list = self.active_list;
        let Some(index) = self.list_extent(list).1 else {
            return;
        };
        if list == ListId::Revisions {
            if let Some(revision) = self.pane.revisions.items.get(index) {
                let id = revision.revision_id.clone();
                self.view_revision(id, cx);
            }
        } else {
            self.select_row(list, index, cx);
        }
    }

    pub(super) fn set_show_deleted(&mut self, on: bool, cx: &mut Context<Self>) {
        self.show_deleted = on;
        self.load_stores(cx);
    }

    pub(super) fn set_all_scopes(&mut self, on: bool, cx: &mut Context<Self>) {
        // The field is only refilled at the next render, so the reload
        // below must not read it.
        let scope = if on { ALL_SCOPES } else { "global" };
        self.fill(Field::Scope, scope);
        let (Some(universe), Some(store)) =
            (self.universe.as_ref().map(|u| u.id), self.store.clone())
        else {
            cx.notify();
            return;
        };
        self.keys.generation += 1;
        let generation = self.keys.generation;
        self.keys.load = Load::Busy;
        cx.notify();
        let at = store_ref(universe, &store, scope);
        let prefix = self.inputs.get(Field::KeySearch, cx).trim().to_string();
        self.request(
            move || {
                fetch::client()?
                    .list_entries(&at, &prefix, None)
                    .map_err(|e| fetch::problem(&e))
            },
            cx,
            move |this, reply, cx| {
                if generation != this.keys.generation {
                    return;
                }
                fill_page(&mut this.keys, reply, false);
                cx.notify();
            },
        );
    }

    pub(super) fn set_entry_tab(&mut self, tab: EntryTab, cx: &mut Context<Self>) {
        self.pane.tab = tab;
        if tab == EntryTab::Versions {
            self.active_list = ListId::Revisions;
            let pane = &self.pane.revisions;
            if pane.items.is_empty() && pane.load == Load::Idle {
                self.load_revisions(false, cx);
            }
        } else {
            self.active_list = ListId::Keys;
        }
        cx.notify();
    }

    pub(super) fn set_value_view(&mut self, view: ValueView, cx: &mut Context<Self>) {
        self.pane.view = view;
        cx.notify();
    }

    pub(super) fn back_to_current(&mut self, cx: &mut Context<Self>) {
        self.pane.revision = None;
        cx.notify();
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        match self.section {
            Section::Standard => {
                self.load_stores(cx);
                if self.store.is_some() {
                    self.load_keys(cx);
                }
            }
            Section::Ordered => {
                if self.ordered.at.is_some() {
                    self.load_ordered(false, cx);
                }
            }
            Section::Memory => {
                if self.memory_kind == MemoryKind::SortedMap && self.map.name.is_some() {
                    self.load_map(false, cx);
                }
            }
        }
    }

    pub(super) fn select_row(&mut self, list: ListId, index: usize, cx: &mut Context<Self>) {
        self.active_list = list;
        match list {
            ListId::Stores => {
                if let Some(store) = self.stores.items.get(index) {
                    let name = store.id.clone();
                    self.select_store(name, cx);
                }
            }
            ListId::Keys => {
                if let Some(entry) = self.keys.items.get(index) {
                    let (scope, key) = (entry.scope.clone(), entry.key.clone());
                    self.select_key(scope, key, cx);
                }
            }
            ListId::Revisions => self.pane.revisions.selected = Some(index),
            ListId::Ordered => self.select_ordered(index, cx),
            ListId::MapItems => self.select_map_item(index, cx),
            ListId::QueueItems => {}
        }
        cx.notify();
    }
}
