//! Several [`Slots`] buffers under one referent index: a pass's batches,
//! keyed by whatever decides which batch an instance draws in (its unit
//! shape, its mesh asset, its mesh-and-skin), with the one entry point a
//! single-instance edit needs — [`Keyed::sync`].

use std::collections::HashMap;

use bytemuck::Pod;
use rbx_dom::Ref;

use super::{Roster, Slots};

/// One batch: its key, whatever else drawing it needs (a file mesh's
/// geometry; nothing for a unit shape), and its instances.
pub(in crate::renderer) struct Group<K, G, T: Pod, S: Copy> {
    pub(in crate::renderer) key: K,
    pub(in crate::renderer) extra: G,
    pub(in crate::renderer) slots: Slots<T, S>,
}

/// The batches of one pass. A group is never dropped once made, even when
/// its last instance leaves — an empty batch draws nothing and costs one
/// small buffer, and keeping it means group positions are stable enough to
/// index by.
pub(in crate::renderer) struct Keyed<K, G, T: Pod, S: Copy> {
    groups: Vec<Group<K, G, T, S>>,
    /// Which group, and which slot in it, holds each referent's instance.
    index: HashMap<Ref, (usize, u32)>,
    label: &'static str,
}

impl<K: PartialEq, G, T: Pod, S: Copy> Keyed<K, G, T, S> {
    pub(in crate::renderer) fn new(label: &'static str) -> Self {
        Keyed {
            groups: Vec::new(),
            index: HashMap::new(),
            label,
        }
    }

    /// Adds a batch built up front (see the callers' `new`), indexing every
    /// instance already in it.
    pub(in crate::renderer) fn add_group(
        &mut self,
        device: &wgpu::Device,
        key: K,
        extra: G,
        roster: Roster<T, S>,
    ) {
        let position = self.groups.len();
        for (offset, &referent) in roster.referents().iter().enumerate() {
            self.index.insert(referent, (position, offset as u32));
        }
        self.groups.push(Group {
            key,
            extra,
            slots: Slots::new(device, self.label, roster),
        });
    }

    pub(in crate::renderer) fn groups(&self) -> &[Group<K, G, T, S>] {
        &self.groups
    }

    /// Takes the batches apart, for a scene rebuild that keeps each one's
    /// payload — a file mesh's vertex buffers — wherever the new scene asks
    /// for the same key again (see `renderer::rebuild`). The instances go
    /// with the old scene; nothing of them is worth keeping.
    pub(in crate::renderer) fn into_groups(self) -> Vec<Group<K, G, T, S>> {
        self.groups
    }

    /// The key of whichever batch currently holds `referent`, for a caller
    /// that has to rebuild a record's key (a new image slot, say) around
    /// whichever part of it did not change — `None` for a referent no batch
    /// holds.
    pub(in crate::renderer) fn key_of(&self, referent: Ref) -> Option<&K> {
        let &(position, _) = self.index.get(&referent)?;
        Some(&self.groups[position].key)
    }

    /// Brings this pass in line with one instance's new state: `wanted` is
    /// the batch it belongs in now and the record to hold there, or `None`
    /// when it no longer belongs in this pass at all. Whichever batch held
    /// it before is left consistent either way — rewritten in place if it is
    /// still the right one, or vacated if not.
    ///
    /// `extra` is asked for a new batch's payload only when the instance
    /// lands in a key no batch has yet; `None` from it means the batch could
    /// not be built (a mesh this renderer never uploaded), and the instance
    /// is left out — the `false` return is the caller's cue to fall back to
    /// a full reload.
    pub(in crate::renderer) fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        referent: Ref,
        wanted: Option<(K, T, S)>,
        extra: impl FnOnce(&K) -> Option<G>,
    ) -> bool {
        let held = self.index.get(&referent).copied();
        match (held, wanted) {
            (Some((position, offset)), Some((key, raw, side)))
                if self.groups[position].key == key =>
            {
                self.groups[position].slots.set(queue, offset, raw, side);
                true
            }
            (Some(_), Some(wanted)) => {
                self.remove(queue, referent);
                self.insert(device, queue, referent, wanted, extra)
            }
            (Some(_), None) => {
                self.remove(queue, referent);
                true
            }
            (None, Some(wanted)) => self.insert(device, queue, referent, wanted, extra),
            (None, None) => true,
        }
    }

    /// Takes `referent` out of whichever batch holds it; a no-op if none does.
    pub(in crate::renderer) fn remove(&mut self, queue: &wgpu::Queue, referent: Ref) {
        let Some((position, offset)) = self.index.remove(&referent) else {
            return;
        };
        if let Some(moved) = self.groups[position].slots.swap_remove(queue, offset) {
            self.index.insert(moved, (position, offset));
        }
    }

    fn insert(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        referent: Ref,
        (key, raw, side): (K, T, S),
        extra: impl FnOnce(&K) -> Option<G>,
    ) -> bool {
        let position = match self.groups.iter().position(|group| group.key == key) {
            Some(position) => position,
            None => {
                let Some(extra) = extra(&key) else {
                    return false;
                };
                self.add_group(device, key, extra, Roster::default());
                self.groups.len() - 1
            }
        };
        let offset = self.groups[position]
            .slots
            .push(device, queue, referent, raw, side);
        self.index.insert(referent, (position, offset));
        true
    }
}
