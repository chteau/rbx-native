//! The Properties-panel fast path for a `Beam` edit: swapping the static
//! definitions the ribbons are rebuilt from every frame, keeping every
//! texture [`Beams::new`] uploaded.

use std::collections::HashMap;

use rbx_assets::AssetRef;

use super::Beams;
use crate::scene::Beam;

impl Beams {
    /// Replaces the beam set with `beams` — the scene's freshly re-planned
    /// list (see `Scene::replan_effect`). Nothing carries over per beam: a
    /// ribbon is rebuilt from its definition every frame (see [`Beams::draw`]),
    /// so the next frame simply draws the new set.
    ///
    /// A beam whose texture this pass has no upload for draws on the flat
    /// white slot — the solid line Roblox itself falls back to — whether that
    /// image failed to decode or has simply not arrived yet. The loader is the
    /// only thing that fetches one (see `load::fetcher`), a `Texture` edit
    /// naming a new one has already asked it to, and the rebuild that follows
    /// the landing is what paints it on.
    pub(in crate::renderer) fn replace(&mut self, beams: &[Beam]) {
        if !self.enabled {
            return;
        }
        self.live = beams
            .iter()
            .map(|beam| {
                (
                    beam.clone(),
                    slot_of(&self.slots, &beam.texture).unwrap_or(0),
                )
            })
            .collect();
    }
}

/// Which upload a beam draws through: the flat-white slot 0 for no texture at
/// all, whatever [`Beams::new`] recorded for a texture it tried (slot 0 again
/// where the download failed), and `None` for one it never tried.
pub(super) fn slot_of(slots: &HashMap<AssetRef, usize>, texture: &AssetRef) -> Option<usize> {
    if *texture == AssetRef::Empty {
        Some(0)
    } else {
        slots.get(texture).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_of_tells_untextured_and_failed_apart_from_never_tried() {
        let slots = HashMap::from([(AssetRef::Id(1), 2), (AssetRef::Id(2), 0)]);

        assert_eq!(slot_of(&slots, &AssetRef::Empty), Some(0), "no texture");
        assert_eq!(slot_of(&slots, &AssetRef::Id(1)), Some(2), "uploaded");
        assert_eq!(slot_of(&slots, &AssetRef::Id(2)), Some(0), "tried, failed");
        assert_eq!(slot_of(&slots, &AssetRef::Id(3)), None, "never tried");
    }
}
