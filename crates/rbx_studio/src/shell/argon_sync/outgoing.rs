//! The outbound half of `shell::argon_sync`: turning the DOM's own change
//! log into one debounced `POST write` per burst of local edits.

use std::collections::HashSet;

use gpui_kit::Context;
use rbx_dom::{Change, Ref, WeakDom};

use crate::argon_client::{self, ArgonRef};

use super::{Shell, SyncDirection, WRITE_DEBOUNCE};

impl Shell {
    // ------------------------------------------------------- write-back

    /// `Shell::reflect_changes`'s own hook (called at the end of that
    /// function): notes which local instances a genuine edit just touched,
    /// then schedules a debounced `POST write` — unless this same batch is
    /// a remote one being applied, see this module's doc comment.
    pub(in crate::shell) fn forward_to_argon(
        &mut self,
        changes: &[Change],
        cx: &mut Context<Self>,
    ) {
        if self.argon.client.is_none() || self.argon.applying || changes.is_empty() {
            return;
        }
        for change in changes {
            match *change {
                Change::Added(referent)
                | Change::Property { referent, .. }
                | Change::Class(referent) => {
                    self.argon.dirty.insert(referent);
                }
                Change::Removed(referent) => {
                    self.argon.dirty.remove(&referent);
                    if let Some(id) = self.argon.ids_rev.remove(&referent) {
                        self.argon.ids.remove(&id);
                        self.argon.removed.push(id);
                    }
                }
                // Argon's `UpdatedSnapshot` has no parent field — a
                // reparent made locally isn't representable as an update
                // on this protocol, so it isn't forwarded. Any property,
                // name or class edit on the same instance still syncs.
                Change::Parent { .. } => {}
            }
        }
        self.schedule_argon_write(cx);
    }

    pub(super) fn schedule_argon_write(&mut self, cx: &mut Context<Self>) {
        self.argon.write_generation = self.argon.write_generation.wrapping_add(1);
        let generation = self.argon.write_generation;
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(WRITE_DEBOUNCE).await;
            let _ = shell.update(cx, |shell, cx| shell.flush_argon_write(generation, cx));
        })
        .detach();
    }

    pub(super) fn flush_argon_write(&mut self, generation: u64, cx: &mut Context<Self>) {
        if generation != self.argon.write_generation {
            return;
        }
        let removals = std::mem::take(&mut self.argon.removed);
        let dirty = ordered_parent_first(&self.dom, std::mem::take(&mut self.argon.dirty));
        let Some(client) = &self.argon.client else {
            return;
        };
        let mut additions = Vec::new();
        let mut updates = Vec::new();
        for referent in dirty {
            let Some(instance) = self.dom.get(referent) else {
                continue;
            };
            let properties: Vec<(String, rmpv::Value)> = instance
                .properties()
                .iter()
                .filter_map(|(name, variant)| {
                    argon_client::encode_value(variant).map(|value| (name.clone(), value))
                })
                .collect();
            match self.argon.ids_rev.get(&referent).copied() {
                Some(id) => updates.push(argon_client::UpdatedSnapshot {
                    id,
                    name: Some(instance.name().to_owned()),
                    class: Some(instance.class().to_owned()),
                    properties: Some(properties),
                }),
                None => {
                    let id = ArgonRef::generate();
                    self.argon.ids.insert(id, referent);
                    self.argon.ids_rev.insert(referent, id);
                    let parent = self
                        .dom
                        .parent(referent)
                        .and_then(|p| self.argon.ids_rev.get(&p).copied())
                        .unwrap_or(ArgonRef::ROOT);
                    additions.push(argon_client::Snapshot {
                        id,
                        parent: Some(parent),
                        name: instance.name().to_owned(),
                        class: instance.class().to_owned(),
                        properties,
                        children: Vec::new(),
                    });
                }
            }
        }
        let changes = argon_client::Changes {
            additions,
            updates,
            removals,
        };
        if !changes.is_empty() {
            client.write(changes);
            self.touch_last_sync(SyncDirection::Up, cx);
        }
    }
}

/// Orders a dirty set so a referent is only emitted once its parent is
/// either already known outside this batch, or already placed earlier in
/// the same batch — a package install (or any multi-instance edit) dirties
/// a parent `Folder`/`ModuleScript` alongside its own children in one go,
/// and `flush_argon_write` needs the parent's `ArgonRef` to exist before it
/// can name it as a child's parent. Draining the `HashSet` directly (as
/// this used to) processes referents in arbitrary hash order, so a child
/// could be visited before its own not-yet-assigned parent and silently
/// fall back to [`ArgonRef::ROOT`] — the wrong place on the Argon side.
///
/// Bounded by the batch's own depth (a handful of iterations for anything
/// this editor would realistically dirty in one edit); a `retain` pass
/// that places nothing at all — which a real tree can't produce, since a
/// root-level referent is always immediately ready — is treated as a
/// malformed edge case rather than looped on forever, and whatever's left
/// is appended in whatever order it was in.
pub(super) fn ordered_parent_first(dom: &WeakDom, dirty: HashSet<Ref>) -> Vec<Ref> {
    let mut remaining: Vec<Ref> = dirty.iter().copied().collect();
    let mut placed: HashSet<Ref> = HashSet::new();
    let mut ordered = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let before = ordered.len();
        remaining.retain(|&referent| {
            let ready = match dom.parent(referent) {
                Some(parent) => !dirty.contains(&parent) || placed.contains(&parent),
                None => true,
            };
            if ready {
                ordered.push(referent);
                placed.insert(referent);
            }
            !ready
        });
        if ordered.len() == before {
            ordered.append(&mut remaining);
            break;
        }
    }
    ordered
}
