//! What the window does in answer to clicks, keys and typing: loading the
//! lists and opening an entry. Moving around and editing are in `nav`, the
//! request plumbing in `requests`, and the writes themselves in `writes`.

use std::time::Duration;

use gpui_kit::component::input::InputEvent;
use gpui_kit::*;

use rbx_cloud::Page;

use super::state::{store_ref, EntryPane, EntryTab, ListId, Listing, Load, Problem};
use super::{fetch, DataStoresWindow, Field};

mod nav;
mod requests;
mod writes;

/// Waiting this long after the last keystroke keeps a search from firing a
/// request per letter.
const TYPING_PAUSE: Duration = Duration::from_millis(250);

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
}
