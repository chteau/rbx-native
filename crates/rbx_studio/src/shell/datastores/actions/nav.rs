//! Moving around the window: selecting rows, the keyboard, sections, tabs
//! and toggles, the experience in use, and opening an entry for editing.

use gpui_kit::*;

use rbx_cloud::ALL_SCOPES;
use serde_json::Value;

use super::super::state::{
    store_ref, EntryDraft, EntryTab, ListId, Load, MemoryKind, Section, TreeTarget, Universe,
    UniverseSource, ValueView,
};
use super::super::{fetch, json_tree, DataStoresWindow, Field};
use super::fill_page;

impl DataStoresWindow {
    pub(in super::super) fn start_edit(&mut self, creating: bool, cx: &mut Context<Self>) {
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

    pub(in super::super) fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.pane.editing = None;
        cx.notify();
    }

    pub(in super::super) fn pick_experience(&mut self, cx: &mut Context<Self>) {
        let window = cx.entity().downgrade();
        // Deferred: not from inside this update, which may be a render's.
        cx.defer(move |cx| {
            let on_pick = {
                let window = window.clone();
                move |experience: rbx_cloud::Experience, _: Option<String>, cx: &mut App| {
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
            let opened = crate::launcher::open_game_picker(on_pick, false, cx);
            let _ = window.update(cx, |this, _| this.picker = opened);
        });
    }

    pub(in super::super) fn use_sandbox(&mut self, cx: &mut Context<Self>) {
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

    pub(in super::super) fn manage_key(&mut self, cx: &mut Context<Self>) {
        cx.defer(|cx| {
            crate::launcher::open_publishing(|_cx| {}, cx);
        });
    }

    pub(in super::super) fn set_section(&mut self, section: Section, cx: &mut Context<Self>) {
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

    pub(in super::super) fn copy(&mut self, text: String, what: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.say(true, format!("Copied {what}."));
        cx.notify();
    }

    pub(in super::super) fn toggle_tree(
        &mut self,
        target: TreeTarget,
        path: String,
        cx: &mut Context<Self>,
    ) {
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

    pub(in super::super) fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
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

    pub(in super::super) fn activate_selection(&mut self, cx: &mut Context<Self>) {
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

    pub(in super::super) fn set_show_deleted(&mut self, on: bool, cx: &mut Context<Self>) {
        self.show_deleted = on;
        self.load_stores(cx);
    }

    pub(in super::super) fn set_all_scopes(&mut self, on: bool, cx: &mut Context<Self>) {
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

    pub(in super::super) fn set_entry_tab(&mut self, tab: EntryTab, cx: &mut Context<Self>) {
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

    pub(in super::super) fn set_value_view(&mut self, view: ValueView, cx: &mut Context<Self>) {
        self.pane.view = view;
        cx.notify();
    }

    pub(in super::super) fn back_to_current(&mut self, cx: &mut Context<Self>) {
        self.pane.revision = None;
        cx.notify();
    }

    pub(in super::super) fn refresh(&mut self, cx: &mut Context<Self>) {
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

    pub(in super::super) fn select_row(
        &mut self,
        list: ListId,
        index: usize,
        cx: &mut Context<Self>,
    ) {
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
