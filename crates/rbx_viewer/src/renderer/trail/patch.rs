//! The Properties-panel fast path for a `Trail` edit: swapping the static
//! definitions under the running recorders, keeping every texture
//! [`Trails::new`] uploaded and every ribbon already laid down.

use std::collections::HashMap;

use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::Trails;
use crate::scene::{Trail, TrailRecorder};

impl Trails {
    /// Replaces the trail set with `trails` — the scene's freshly re-planned
    /// list (see `Scene::replan_effect`) — keeping the recorder (the drawn
    /// history) of every trail still present, so an edit never wipes the
    /// ribbon it has already laid down; a trail that just appeared starts
    /// with an empty history, exactly as at load.
    ///
    /// A trail whose texture this pass has no upload for draws on the flat
    /// white slot — the solid plane Roblox itself falls back to — for the same
    /// reasons as `renderer::beam::Beams::replace`.
    pub(in crate::renderer) fn replace(&mut self, trails: &[Trail]) {
        if !self.enabled {
            return;
        }
        let previous = std::mem::take(&mut self.live);
        self.live = carry_over(previous, trails, |texture| {
            slot_of(&self.slots, texture).unwrap_or(0)
        });
    }
}

/// Same contract as `renderer::beam::patch::slot_of`: slot 0 for no texture
/// or a failed one, `None` for a texture never tried.
pub(super) fn slot_of(slots: &HashMap<AssetRef, usize>, texture: &AssetRef) -> Option<usize> {
    if *texture == AssetRef::Empty {
        Some(0)
    } else {
        slots.get(texture).copied()
    }
}

/// [`Trails::replace`]'s pure half: every trail in `trails`, paired with its
/// own recorder out of `previous` where its referent was already live and
/// with an empty one otherwise.
fn carry_over(
    previous: Vec<(Trail, TrailRecorder, usize)>,
    trails: &[Trail],
    slot: impl Fn(&AssetRef) -> usize,
) -> Vec<(Trail, TrailRecorder, usize)> {
    let mut recorders: HashMap<Ref, TrailRecorder> = previous
        .into_iter()
        .map(|(trail, recorder, _)| (trail.referent, recorder))
        .collect();
    trails
        .iter()
        .map(|trail| {
            let recorder = recorders.remove(&trail.referent).unwrap_or_default();
            (trail.clone(), recorder, slot(&trail.texture))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use rbx_dom::{ColorSequence, NumberSequence};

    use super::*;

    fn trail(referent: u32) -> Trail {
        Trail {
            position0: Vec3::ZERO,
            position1: Vec3::X,
            enabled: true,
            lifetime: 2.0,
            min_length: 0.0,
            width_scale: NumberSequence {
                keypoints: Vec::new(),
            },
            color: ColorSequence {
                keypoints: Vec::new(),
            },
            transparency: NumberSequence {
                keypoints: Vec::new(),
            },
            texture: AssetRef::Empty,
            texture_length: 1.0,
            light_emission: 0.0,
            referent: Ref::new(referent),
        }
    }

    // A `Lifetime` or `Color` edit must not erase the ribbon already drawn:
    // the history belongs to the trail, whichever definition it draws with.
    #[test]
    fn a_surviving_trail_keeps_its_history() {
        let mut recorder = TrailRecorder::new();
        recorder.record(0.0, Vec3::ZERO, Vec3::X, 0.0);
        recorder.record(0.5, Vec3::Z, Vec3::X + Vec3::Z, 0.0);
        assert_eq!(recorder.samples().len(), 2);

        let edited = Trail {
            lifetime: 9.0,
            ..trail(4)
        };
        let live = carry_over(vec![(trail(4), recorder, 0)], &[edited], |_| 0);

        assert_eq!(live.len(), 1);
        assert_eq!(live[0].0.lifetime, 9.0);
        assert_eq!(live[0].1.samples().len(), 2);
    }

    #[test]
    fn a_new_trail_starts_with_an_empty_history() {
        let live = carry_over(Vec::new(), &[trail(1)], |_| 0);

        assert_eq!(live.len(), 1);
        assert!(live[0].1.samples().is_empty());
    }
}
