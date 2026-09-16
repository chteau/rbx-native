//! One instance buffer that a single instance can be added to or taken out
//! of without rebuilding the rest: what lets a Properties-panel edit that
//! moves a part between GPU batches (opaque to blended, box to ball, caster
//! to non-caster) stay a single-instance operation instead of a full reload.
//!
//! [`Roster`] is the CPU mirror — every referent, its GPU record and any
//! CPU-only sidecar (a cull sphere, say) in buffer order — and the only part
//! with any bookkeeping to get wrong, so it is the part with tests. [`Slots`]
//! wraps one in the `wgpu::Buffer` it mirrors; [`keyed::Keyed`] groups several
//! under a batch key and keeps the referent index across them.

pub(super) mod keyed;

use bytemuck::Pod;
use rbx_dom::Ref;
use wgpu::util::DeviceExt;

/// Every instance of one buffer, in buffer order, with what each one is
/// the instance *of*.
///
/// Removal is a swap-remove: the last instance drops into the freed slot
/// rather than everything after it shifting down, so a removal costs one
/// buffer write and one index update however long the buffer is. The price
/// is that buffer order is not DOM order — nothing here relies on it; the
/// blended passes (which do care about order) re-sort every frame anyway.
pub(super) struct Roster<T, S = ()> {
    referents: Vec<Ref>,
    raw: Vec<T>,
    side: Vec<S>,
}

impl<T, S> Default for Roster<T, S> {
    fn default() -> Self {
        Roster {
            referents: Vec::new(),
            raw: Vec::new(),
            side: Vec::new(),
        }
    }
}

impl<T: Copy, S: Copy> Roster<T, S> {
    pub(super) fn from_iter(entries: impl IntoIterator<Item = (Ref, T, S)>) -> Self {
        let mut roster = Roster::default();
        for (referent, raw, side) in entries {
            roster.push(referent, raw, side);
        }
        roster
    }

    pub(super) fn len(&self) -> usize {
        self.referents.len()
    }

    pub(super) fn raw(&self) -> &[T] {
        &self.raw
    }

    pub(super) fn referents(&self) -> &[Ref] {
        &self.referents
    }

    pub(super) fn side(&self, offset: usize) -> S {
        self.side[offset]
    }

    pub(super) fn set(&mut self, offset: usize, raw: T, side: S) {
        self.raw[offset] = raw;
        self.side[offset] = side;
    }

    /// Appends, returning the slot it landed in.
    pub(super) fn push(&mut self, referent: Ref, raw: T, side: S) -> usize {
        self.referents.push(referent);
        self.raw.push(raw);
        self.side.push(side);
        self.referents.len() - 1
    }

    /// Frees `offset` by dropping the last instance into it, and names the
    /// referent that just moved there so the caller can re-index it — `None`
    /// when `offset` *was* the last one and nothing moved.
    pub(super) fn swap_remove(&mut self, offset: usize) -> Option<Ref> {
        self.referents.swap_remove(offset);
        self.raw.swap_remove(offset);
        self.side.swap_remove(offset);
        (offset < self.referents.len()).then(|| self.referents[offset])
    }
}

/// A [`Roster`] and the vertex buffer holding its GPU records.
///
/// The buffer is allocated with slack and only `count()` instances of it are
/// ever drawn, so a push that fits is one slot's write; one that does not
/// reallocates at double the size and re-uploads the roster — rare enough
/// (a bucket crossing is one instance per edit) that it is not worth a
/// smarter growth policy.
///
/// Nothing here writes the buffer as it goes: an edit marks the slots it
/// touched and [`Slots::flush`], run once before the next frame reads the
/// buffer, uploads the span between the lowest and the highest of them in
/// one `write_buffer`. A script moving a hundred parts is then one upload
/// per batch rather than a hundred staged copies of 96 bytes each, and a
/// single-part edit still costs one. The roster is the buffer's mirror, so
/// the untouched slots inside the span are rewritten with what they already
/// hold.
pub(super) struct Slots<T: Pod, S: Copy = ()> {
    roster: Roster<T, S>,
    buffer: wgpu::Buffer,
    /// In instances, not bytes.
    capacity: usize,
    label: &'static str,
    /// The slots whose GPU copy is behind the roster, as `[first, last]`.
    dirty: Option<(usize, usize)>,
}

const USAGE: wgpu::BufferUsages = wgpu::BufferUsages::VERTEX.union(wgpu::BufferUsages::COPY_DST);

impl<T: Pod, S: Copy> Slots<T, S> {
    pub(super) fn new(device: &wgpu::Device, label: &'static str, roster: Roster<T, S>) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(roster.raw()),
            usage: USAGE,
        });
        Slots {
            capacity: roster.len(),
            roster,
            buffer,
            label,
            dirty: None,
        }
    }

    pub(super) fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub(super) fn count(&self) -> u32 {
        self.roster.len() as u32
    }

    pub(super) fn side(&self, offset: u32) -> S {
        self.roster.side(offset as usize)
    }

    pub(super) fn set(&mut self, offset: u32, raw: T, side: S) {
        self.roster.set(offset as usize, raw, side);
        self.mark(offset as usize, offset as usize);
    }

    /// See [`Roster::push`]; reallocates when the buffer is full, in which
    /// case the whole roster is owed to the new buffer.
    pub(super) fn push(&mut self, device: &wgpu::Device, referent: Ref, raw: T, side: S) -> u32 {
        let offset = self.roster.push(referent, raw, side);
        if self.roster.len() > self.capacity {
            self.capacity = (self.capacity * 2).max(self.roster.len());
            self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: stride_of::<T>(self.capacity as u32),
                usage: USAGE,
                mapped_at_creation: false,
            });
            self.dirty = Some((0, offset));
        } else {
            self.mark(offset, offset);
        }
        offset as u32
    }

    /// See [`Roster::swap_remove`]; the moved record is owed to the freed
    /// slot, and the stale copy past `count()` is simply never drawn.
    pub(super) fn swap_remove(&mut self, offset: u32) -> Option<Ref> {
        let moved = self.roster.swap_remove(offset as usize)?;
        self.mark(offset as usize, offset as usize);
        Some(moved)
    }

    /// Uploads every slot an edit touched since the last flush, as one
    /// write of the span they lie in. Must run before a pass reads the
    /// buffer — `Renderer::draw` does, first thing.
    pub(super) fn flush(&mut self, queue: &wgpu::Queue) {
        let Some(span) = self
            .dirty
            .take()
            .and_then(|dirty| span(dirty, self.roster.len()))
        else {
            return;
        };
        queue.write_buffer(
            &self.buffer,
            stride_of::<T>(span.start as u32),
            bytemuck::cast_slice(&self.roster.raw()[span]),
        );
    }

    fn mark(&mut self, first: usize, last: usize) {
        self.dirty = Some(widened(self.dirty, first, last));
    }
}

/// The dirty span grown to take in slots `first..=last`.
fn widened(dirty: Option<(usize, usize)>, first: usize, last: usize) -> (usize, usize) {
    match dirty {
        Some((lo, hi)) => (lo.min(first), hi.max(last)),
        None => (first, last),
    }
}

/// The slots a flush actually writes for a dirty `(first, last)` over a
/// roster `len` long: the span clipped to the roster, since a removal
/// marks the slot it vacated and that slot may since have fallen off the
/// end — or the roster may have emptied altogether, in which case nothing
/// is owed. `None` for nothing to write.
fn span((first, last): (usize, usize), len: usize) -> Option<std::ops::Range<usize>> {
    (first < len).then(|| first..last.min(len - 1) + 1)
}

fn stride_of<T>(instances: u32) -> wgpu::BufferAddress {
    u64::from(instances) * std::mem::size_of::<T>() as wgpu::BufferAddress
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster(referents: impl IntoIterator<Item = u32>) -> Roster<u32, ()> {
        Roster::from_iter(referents.into_iter().map(|id| (Ref::new(id), id * 10, ())))
    }

    fn ids(roster: &Roster<u32, ()>) -> Vec<u32> {
        roster.referents().iter().map(|r| r.value()).collect()
    }

    #[test]
    fn pushing_appends_in_order_and_reports_the_slot() {
        let mut roster = roster([1, 2]);

        assert_eq!(roster.push(Ref::new(3), 30, ()), 2);
        assert_eq!(ids(&roster), vec![1, 2, 3]);
        assert_eq!(roster.raw(), &[10, 20, 30]);
    }

    // The freed slot is refilled from the end, and the caller is told which
    // referent now lives there — an index still pointing it at the old end
    // would draw the wrong instance, or read past `len`.
    #[test]
    fn removing_from_the_middle_moves_the_last_one_in_and_names_it() {
        let mut roster = roster([1, 2, 3, 4]);

        assert_eq!(roster.swap_remove(1), Some(Ref::new(4)));

        assert_eq!(ids(&roster), vec![1, 4, 3]);
        assert_eq!(roster.raw(), &[10, 40, 30]);
        assert_eq!(roster.len(), 3);
    }

    #[test]
    fn removing_the_last_one_moves_nothing() {
        let mut roster = roster([1, 2, 3]);

        assert_eq!(roster.swap_remove(2), None);
        assert_eq!(ids(&roster), vec![1, 2]);
    }

    #[test]
    fn removing_the_only_one_leaves_it_empty() {
        let mut roster = roster([7]);

        assert_eq!(roster.swap_remove(0), None);
        assert_eq!(roster.len(), 0);
    }

    // The span a flush uploads: the lowest and highest slot touched, the
    // untouched ones between them included (they are rewritten with what
    // they hold).
    #[test]
    fn a_dirty_span_covers_every_touched_slot() {
        let mut dirty = None;
        for slot in [4, 1, 7] {
            dirty = Some(widened(dirty, slot, slot));
        }
        assert_eq!(dirty, Some((1, 7)));
        assert_eq!(span((1, 7), 10), Some(1..8));
    }

    // A removal marks the slot it vacated, which may have fallen off the
    // end by flush time — and a batch's last instance leaving is a removal
    // that marks slot 0 of a roster with nothing left to write.
    #[test]
    fn a_span_is_clipped_to_the_roster_and_an_empty_roster_owes_nothing() {
        assert_eq!(span((1, 7), 5), Some(1..5));
        assert_eq!(span((4, 4), 4), None, "the marked slot itself fell off");
        assert_eq!(span((0, 0), 0), None);
        assert_eq!(span((0, 3), 1), Some(0..1));
    }

    // The sidecar has to travel with its record through every operation, or
    // a cull sphere ends up guarding a different instance than it was
    // computed for.
    #[test]
    fn the_sidecar_follows_its_record() {
        let mut roster: Roster<u32, char> = Roster::from_iter([
            (Ref::new(1), 1, 'a'),
            (Ref::new(2), 2, 'b'),
            (Ref::new(3), 3, 'c'),
        ]);

        roster.set(0, 11, 'A');
        roster.swap_remove(1);

        assert_eq!(roster.side(0), 'A');
        assert_eq!(roster.side(1), 'c');
        assert_eq!(roster.raw(), &[11, 3]);
    }
}
