//! `RBX_STUDIO_DATASTORE_OPEN="key=value;key=value"` lands the window on a
//! given state, so a capture script needs no clicks.
//!
//! Keys: `section` (standard|ordered|memory), `store`, `key`, `tab`
//! (value|versions), `view` (tree|raw), `edit`, `create`, `dialog`
//! (save|delete|increment|rollback|conflict), `ordered`, `map`, `map_item`,
//! `memory` (map|queue), `queue`, `read`, `add`. Flags are `1` or `true`.
//! Unknown keys are ignored.

use gpui_kit::Context;
use serde_json::{json, Value};

use rbx_cloud::EntryWrite;

use super::state::{
    store_ref, Dialog, EntryTab, ListId, Load, MemoryKind, Pending, Section, ValueView,
};
use super::DataStoresWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DebugDialog {
    Save,
    Delete,
    Increment,
    Rollback,
    Conflict,
}

/// What is still to be done; each step clears its own field once done.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct DebugOpen {
    pub(super) section: Option<Section>,
    pub(super) memory: Option<MemoryKind>,
    pub(super) store: Option<String>,
    pub(super) key: Option<String>,
    pub(super) tab: Option<EntryTab>,
    pub(super) view: Option<ValueView>,
    pub(super) edit: bool,
    pub(super) create: bool,
    pub(super) dialog: Option<DebugDialog>,
    pub(super) ordered: Option<String>,
    pub(super) map: Option<String>,
    pub(super) map_item: Option<usize>,
    pub(super) queue: Option<String>,
    pub(super) read: bool,
    pub(super) add: bool,
    /// Versions are asked for once, so an empty history doesn't loop.
    revisions_asked: bool,
}

fn flag(value: &str) -> bool {
    matches!(value, "1" | "true")
}

impl DebugOpen {
    pub(super) fn from_env() -> Option<Self> {
        let spec = std::env::var("RBX_STUDIO_DATASTORE_OPEN").ok()?;
        (!spec.trim().is_empty()).then(|| Self::parse(&spec))
    }

    pub(super) fn parse(spec: &str) -> Self {
        let mut open = DebugOpen::default();
        for pair in spec.split(';') {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let (name, value) = (name.trim(), value.trim());
            let text = || Some(value.to_string()).filter(|v| !v.is_empty());
            match name {
                "section" => {
                    open.section = match value {
                        "standard" => Some(Section::Standard),
                        "ordered" => Some(Section::Ordered),
                        "memory" => Some(Section::Memory),
                        _ => None,
                    }
                }
                "store" => open.store = text(),
                "key" => open.key = text(),
                "tab" => {
                    open.tab = match value {
                        "value" => Some(EntryTab::Value),
                        "versions" => Some(EntryTab::Versions),
                        _ => None,
                    }
                }
                "view" => {
                    open.view = match value {
                        "tree" => Some(ValueView::Tree),
                        "raw" => Some(ValueView::Raw),
                        _ => None,
                    }
                }
                "edit" => open.edit = flag(value),
                "create" => open.create = flag(value),
                "dialog" => {
                    open.dialog = match value {
                        "save" => Some(DebugDialog::Save),
                        "delete" => Some(DebugDialog::Delete),
                        "increment" => Some(DebugDialog::Increment),
                        "rollback" => Some(DebugDialog::Rollback),
                        "conflict" => Some(DebugDialog::Conflict),
                        _ => None,
                    }
                }
                "ordered" => open.ordered = text(),
                "map" => open.map = text(),
                "map_item" => open.map_item = value.parse().ok(),
                "memory" => {
                    open.memory = match value {
                        "map" => Some(MemoryKind::SortedMap),
                        "queue" => Some(MemoryKind::Queue),
                        _ => None,
                    }
                }
                "queue" => open.queue = text(),
                "read" => open.read = flag(value),
                "add" => open.add = flag(value),
                _ => {}
            }
        }
        open
    }
}

impl DataStoresWindow {
    /// Does whatever `self.debug` has become due, consuming each step.
    pub(super) fn debug_step(&mut self, cx: &mut Context<Self>) {
        let Some(mut d) = self.debug.take() else {
            return;
        };
        self.debug_navigate(&mut d, cx);
        self.debug_entry(&mut d, cx);
        self.debug_memory(&mut d, cx);
        if d != DebugOpen::default() {
            self.debug = Some(d);
        }
    }

    fn debug_navigate(&mut self, d: &mut DebugOpen, cx: &mut Context<Self>) {
        if let Some(section) = d.section.take() {
            self.set_section(section, cx);
        }
        if let Some(kind) = d.memory.take() {
            self.set_memory_kind(kind, cx);
        }
        if d.store.is_some() && self.stores.load == Load::Idle {
            if let Some(name) = d.store.take() {
                if self.stores.items.iter().any(|s| s.id == name) {
                    self.select_store(name, cx);
                }
            }
        }
        if d.key.is_some() && self.store.is_some() && self.keys.load == Load::Idle {
            let wanted = d.key.clone().unwrap_or_default();
            if let Some(index) = self.keys.items.iter().position(|k| k.key == wanted) {
                d.key = None;
                self.select_row(ListId::Keys, index, cx);
            } else if self.keys.next.is_none() {
                d.key = None;
            }
        }
    }

    fn debug_entry(&mut self, d: &mut DebugOpen, cx: &mut Context<Self>) {
        if self.pane.load != Load::Idle {
            return;
        }
        let Some(entry) = self.pane.entry.clone() else {
            return;
        };
        let (Some(universe), Some(store)) =
            (self.universe.as_ref().map(|u| u.id), self.store.clone())
        else {
            return;
        };
        if let Some(view) = d.view.take() {
            self.set_value_view(view, cx);
        }
        if let Some(tab) = d.tab.take() {
            self.set_entry_tab(tab, cx);
        }
        if std::mem::take(&mut d.edit) {
            self.start_edit(false, cx);
        }
        if std::mem::take(&mut d.create) {
            self.start_edit(true, cx);
        }
        let at = store_ref(universe, &store, &entry.scope);
        let key = entry.key.clone();
        let Some(kind) = d.dialog else {
            return;
        };
        match kind {
            DebugDialog::Save => {
                let write = EntryWrite {
                    value: entry.value.clone(),
                    users: entry.users.clone(),
                    attributes: entry.attributes.clone(),
                    etag: Some(entry.etag.clone()),
                };
                self.ask(
                    Pending::SaveEntry {
                        at,
                        key,
                        write,
                        creating: false,
                    },
                    cx,
                );
            }
            DebugDialog::Delete => self.ask_delete(cx),
            DebugDialog::Increment => {
                self.ask(Pending::IncrementEntry { at, key, amount: 5 }, cx);
            }
            DebugDialog::Rollback => {
                let revisions = &self.pane.revisions;
                if revisions.items.is_empty() {
                    if revisions.load == Load::Idle && !d.revisions_asked {
                        d.revisions_asked = true;
                        self.load_revisions(false, cx);
                    } else if revisions.load == Load::Idle {
                        d.dialog = None;
                    }
                    return;
                }
                if revisions.load != Load::Idle {
                    return;
                }
                if let Some(oldest) = revisions.items.last().cloned() {
                    self.ask_rollback(oldest, cx);
                }
            }
            DebugDialog::Conflict => {
                let mut newer = entry.clone();
                let mut value = match &entry.value {
                    Value::Object(map) => map.clone(),
                    _ => serde_json::Map::new(),
                };
                if let Value::Object(extra) = json!({ "coins": 999_999 }) {
                    value.extend(extra);
                }
                newer.value = Value::Object(value);
                newer.etag = "newer-etag".to_string();
                let write = EntryWrite {
                    value: entry.value.clone(),
                    users: entry.users.clone(),
                    attributes: entry.attributes.clone(),
                    etag: Some(entry.etag.clone()),
                };
                self.dialog = Some(Dialog::Conflict {
                    pending: Pending::SaveEntry {
                        at,
                        key,
                        write,
                        creating: false,
                    },
                    newer: Box::new(newer),
                });
            }
        }
        d.dialog = None;
        cx.notify();
    }

    fn debug_memory(&mut self, d: &mut DebugOpen, cx: &mut Context<Self>) {
        if self.universe.is_none() {
            return;
        }
        if let Some(name) = d.ordered.take() {
            self.open_ordered_named(name, cx);
        }
        if let Some(name) = d.map.take() {
            self.open_map_named(name, cx);
        }
        if let Some(index) = d.map_item {
            if self.map.name.is_some() && self.map.list.load == Load::Idle {
                d.map_item = None;
                if index < self.map.list.items.len() {
                    self.select_row(ListId::MapItems, index, cx);
                }
            }
        }
        if let Some(name) = d.queue.take() {
            self.open_queue_named(name, cx);
        }
        if self.queue.name.is_some() {
            if std::mem::take(&mut d.read) {
                self.read_queue(cx);
            }
            if std::mem::take(&mut d.add) {
                self.start_queue_add(true, cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_keys() {
        let d = DebugOpen::parse(
            "section=memory; store=PlayerData;key=player_1;tab=versions;view=raw;edit=1;dialog=conflict;map_item=2;memory=queue;read=true",
        );
        assert_eq!(d.section, Some(Section::Memory));
        assert_eq!(d.store.as_deref(), Some("PlayerData"));
        assert_eq!(d.key.as_deref(), Some("player_1"));
        assert_eq!(d.tab, Some(EntryTab::Versions));
        assert_eq!(d.view, Some(ValueView::Raw));
        assert!(d.edit && d.read && !d.create && !d.add);
        assert_eq!(d.dialog, Some(DebugDialog::Conflict));
        assert_eq!(d.map_item, Some(2));
        assert_eq!(d.memory, Some(MemoryKind::Queue));
    }

    #[test]
    fn ignores_unknown_and_malformed() {
        let d = DebugOpen::parse("bogus=1;section=nowhere;novalue;map_item=x;;");
        assert_eq!(d, DebugOpen::default());
    }
}
