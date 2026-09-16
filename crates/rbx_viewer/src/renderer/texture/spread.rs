//! A bounded-per-tick queue for GPU texture uploads deferred out of scene
//! load, so a place with many `Decal`/`Texture` images doesn't stall one
//! frame uploading all of them at once (see [`Textured::upload_pending`]).
//!
//! [`Textured::upload_pending`]: super::super::textured::Textured::upload_pending

use std::collections::VecDeque;

/// How many images [`Pending::take`] releases per call.
///
/// A fixed count rather than a wall-clock time budget: actual GPU upload
/// cost depends on driver/hardware in a way this crate can't observe without
/// a blocking readback — which would itself stall the very frame this budget
/// exists to protect — while a count is exactly what a caller (and a test)
/// can reason about deterministically. Paired with the existing
/// `quality::MAX_TEXTURE_SIZE` cap on any single texture, this bounds
/// worst-case per-frame upload cost without needing to measure it directly.
/// Sized so a place with a handful of textures still finishes loading within
/// a couple of frames, while one with hundreds spreads the cost over a few
/// seconds of frames rather than a single stalled one.
pub(in crate::renderer) const PER_FRAME: usize = 8;

/// Upload work not yet sent to the GPU, drained a bounded amount at a time.
///
/// Each item carries its own destination (a texture slot index, in
/// [`Textured`](super::super::textured::Textured)) rather than relying on
/// queue position to mean anything downstream, so draining out of order — or
/// only partially, frame after frame — never mismatches an image with the
/// wrong GPU resource.
pub(in crate::renderer) struct Pending<T> {
    items: VecDeque<T>,
}

impl<T> Pending<T> {
    pub(in crate::renderer) fn new(items: impl IntoIterator<Item = T>) -> Self {
        Pending {
            items: items.into_iter().collect(),
        }
    }

    pub(in crate::renderer) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Removes and returns up to `budget` items, oldest first. Pass
    /// [`usize::MAX`] to drain everything at once — what a caller with no
    /// next frame to spread the rest across (the single-shot `--screenshot`
    /// path) needs instead of a half-finished load.
    pub(in crate::renderer) fn take(&mut self, budget: usize) -> Vec<T> {
        (0..budget.min(self.items.len()))
            .filter_map(|_| self.items.pop_front())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_never_returns_more_than_its_budget() {
        let mut pending = Pending::new(0..10);
        assert_eq!(pending.take(3), vec![0, 1, 2]);
        assert_eq!(pending.take(3), vec![3, 4, 5]);
    }

    #[test]
    fn take_returns_fewer_once_the_queue_runs_low() {
        let mut pending = Pending::new(0..2);
        assert_eq!(pending.take(8), vec![0, 1]);
        assert!(pending.take(8).is_empty());
    }

    #[test]
    fn every_item_is_eventually_taken_exactly_once() {
        let mut pending = Pending::new(0..37);
        let mut seen = Vec::new();
        while !pending.is_empty() {
            seen.extend(pending.take(4));
        }
        assert_eq!(seen, (0..37).collect::<Vec<_>>());
    }

    #[test]
    fn take_preserves_first_in_first_out_order() {
        // Each item stands for (slot, image) in the real caller; order here
        // is what keeps a spread-out upload from landing on the wrong slot.
        let mut pending = Pending::new(["a", "b", "c", "d", "e"]);
        assert_eq!(pending.take(2), vec!["a", "b"]);
        assert_eq!(pending.take(2), vec!["c", "d"]);
        assert_eq!(pending.take(2), vec!["e"]);
    }

    #[test]
    fn usize_max_drains_everything_in_one_call() {
        let mut pending = Pending::new(0..50);
        assert_eq!(pending.take(usize::MAX).len(), 50);
        assert!(pending.is_empty());
    }
}
