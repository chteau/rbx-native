//! The inbound half of `shell::argon_sync`: writing a server batch —
//! the initial snapshot, or a later `Changes` — into the DOM as one undo
//! step, with the id table kept in step.

use gpui_kit::Context;
use rbx_dom::{Ref, WeakDom};

use crate::argon_client::{self, ArgonRef};

use super::{PendingReview, Shell, SyncDirection, REVIEW_THRESHOLD};

impl Shell {
    pub(super) fn handle_incoming_changes(
        &mut self,
        changes: argon_client::Changes,
        cx: &mut Context<Self>,
    ) {
        if changes.is_empty() {
            return;
        }
        if changes.len() >= REVIEW_THRESHOLD {
            self.argon.pending = Some(PendingReview {
                additions: changes.additions.len(),
                updates: changes.updates.len(),
                removals: changes.removals.len(),
                changes,
            });
            cx.notify();
            return;
        }
        self.apply_argon_changes(changes, cx);
        self.touch_last_sync(SyncDirection::Down, cx);
    }

    // ---------------------------------------------------------- apply path

    pub(super) fn apply_initial_snapshot(
        &mut self,
        snapshot: argon_client::Snapshot,
        cx: &mut Context<Self>,
    ) {
        self.push_history();
        self.argon.applying = true;
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        for child in snapshot.children {
            self.apply_snapshot_node(&mut dom, child, None);
        }
        self.dom = dom;
        let log = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&log, cx);
        self.argon.applying = false;
        self.record_history_change(log);
        cx.notify();
    }

    pub(super) fn apply_argon_changes(
        &mut self,
        changes: argon_client::Changes,
        cx: &mut Context<Self>,
    ) {
        if changes.is_empty() {
            return;
        }
        self.push_history();
        self.argon.applying = true;
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        for addition in changes.additions {
            self.apply_addition(&mut dom, addition);
        }
        for update in changes.updates {
            self.apply_update(&mut dom, update);
        }
        for removal in changes.removals {
            self.apply_removal(&mut dom, removal);
        }
        self.dom = dom;
        let log = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&log, cx);
        self.argon.applying = false;
        self.record_history_change(log);
        cx.notify();
    }

    /// One `additions` entry: resolves its `parent` id against what this
    /// client already knows, and drops the entry (rather than guessing) if
    /// that parent hasn't been seen yet — a later full resync (or the
    /// parent's own addition, if it's in the same batch and ordered first,
    /// which `argon-roblox` itself doesn't guarantee) will bring it in.
    pub(super) fn apply_addition(&mut self, dom: &mut WeakDom, node: argon_client::Snapshot) {
        match node.parent {
            Some(ArgonRef::ROOT) | None => {
                self.apply_snapshot_node(dom, node, None);
            }
            Some(parent_id) => {
                if let Some(&parent) = self.argon.ids.get(&parent_id) {
                    self.apply_snapshot_node(dom, node, Some(parent));
                }
            }
        }
    }

    /// One instance, recursively. With no local parent given, this is a
    /// root-level entry — reused against an existing service of the same
    /// class rather than duplicated, the same special case
    /// `argon-roblox`'s own `Processor.Write` makes for `game:
    /// FindFirstChildOfClass`.
    pub(super) fn apply_snapshot_node(
        &mut self,
        dom: &mut WeakDom,
        node: argon_client::Snapshot,
        parent: Option<Ref>,
    ) -> Ref {
        let existing = parent.is_none().then(|| {
            dom.root_refs()
                .iter()
                .copied()
                .find(|&r| dom.get(r).is_some_and(|i| i.class() == node.class))
        });
        let referent = match existing.flatten() {
            Some(referent) => {
                if dom.get(referent).is_some_and(|i| i.name() != node.name) {
                    let _ = dom.set_name(referent, &node.name);
                }
                referent
            }
            None => dom.new_instance(&node.class, &node.name, parent),
        };
        self.argon.ids.insert(node.id, referent);
        self.argon.ids_rev.insert(referent, node.id);
        for (name, encoded) in &node.properties {
            if let Some(variant) = argon_client::decode_value(encoded) {
                let _ = dom.set_property(referent, name, variant);
            }
        }
        for child in node.children {
            self.apply_snapshot_node(dom, child, Some(referent));
        }
        referent
    }

    pub(super) fn apply_update(
        &mut self,
        dom: &mut WeakDom,
        update: argon_client::UpdatedSnapshot,
    ) {
        let Some(&referent) = self.argon.ids.get(&update.id) else {
            return;
        };
        if dom.get(referent).is_none() {
            return;
        }
        if let Some(name) = &update.name {
            let _ = dom.set_name(referent, name);
        }
        if let Some(class) = &update.class {
            let _ = dom.set_class(referent, class);
        }
        // v1 simplification: properties present in the update are written;
        // ones the update omits are left as they stand rather than reset
        // to their class default (which `argon-roblox`'s own non-initial
        // sync path does) — see the Argon sync plan's known simplifications.
        if let Some(properties) = &update.properties {
            for (name, encoded) in properties {
                if let Some(variant) = argon_client::decode_value(encoded) {
                    let _ = dom.set_property(referent, name, variant);
                }
            }
        }
    }

    pub(super) fn apply_removal(&mut self, dom: &mut WeakDom, id: ArgonRef) {
        let Some(&referent) = self.argon.ids.get(&id) else {
            return;
        };
        for removed in dom.remove(referent) {
            if let Some(removed_id) = self.argon.ids_rev.remove(&removed) {
                self.argon.ids.remove(&removed_id);
            }
        }
    }
}
