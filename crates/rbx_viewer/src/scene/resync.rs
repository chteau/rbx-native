//! Bringing one `BasePart` of a built scene in line with the DOM — added,
//! edited, moved or gone — see [`Scene::resync_part`].

mod pieces;

use std::ops::Range;

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::patch::Replanned;
use super::{
    bounds, build_part, is_drawable, Part, Placement, Scene, UnionEvaluations, WORKSPACE_CLASS,
};
use crate::changes::Rebuild;

/// What the renderer now has to show for one referent after
/// [`Scene::resync_part`] — every pass either rewrites its record for it or
/// takes it out, whichever [`Drawn`] says, plus the piece slots the referent
/// has stopped filling.
#[derive(Debug, Clone)]
pub(crate) struct PartSync {
    pub(crate) drawn: Drawn,
    /// The recovered pieces (see [`PartId`]) this scene drew `referent` as
    /// before the resync and does not draw it as now: a union whose asset
    /// changed under it, or one that stopped being drawn as pieces at all.
    /// Empty for everything else, which is nearly every edit.
    pub(crate) dropped: Range<u32>,
}

/// What one referent draws as now.
#[derive(Debug, Clone)]
pub(crate) enum Drawn {
    /// Its own unit-shape box, `Part::is_drawn` permitting: a fully
    /// transparent box still keeps its placement, for the decals on it.
    Box(Part),
    /// A resolved file mesh or computed union mesh:
    /// `Scene::resolved_file_meshes`'s `instances[index]`. Its box is
    /// suppressed, and with it its placement.
    Mesh(usize),
    /// The additive pieces recovered from a union whose boolean failed (see
    /// `scene::union`), each its own instance in every box pass. `placement`
    /// is the union's own box, which is not drawn but is still what an
    /// outline is drawn around and what a decal is projected on — see
    /// [`Scene::placements`].
    Pieces {
        placement: Placement,
        pieces: Vec<Part>,
    },
    /// Nothing drawn for it any more: not in the DOM, not under `Workspace`,
    /// not a drawable class, without a size or a frame, or a mesh part that
    /// turned invisible.
    Gone,
}

impl PartSync {
    /// Nothing drawn for `referent`, and `dropped` pieces to take out with
    /// it — what a removal hands the renderer.
    pub(crate) fn gone(dropped: Range<u32>) -> Self {
        PartSync {
            drawn: Drawn::Gone,
            dropped,
        }
    }
}

impl Scene {
    /// Recomputes one part from `dom`, exactly as [`Scene::from_dom`] and
    /// the resolutions after it would build it, and puts the result in
    /// place of whatever this scene held for `referent` — which may be
    /// nothing at all (an insert, a `Model` moved into `Workspace`), a box,
    /// a resolved mesh or a union's recovered pieces, and may become any of
    /// the four.
    ///
    /// Everything is read back from the DOM rather than taken from an edit:
    /// an undo hands over the log its mutation produced, and the DOM, not
    /// the log, is the truth about what the referent is now.
    ///
    /// `known_layers` is the material catalog's layer count as of the last
    /// full build — how many texture-array layers the renderer holds. An
    /// edit that lands on a material past that count needs its maps
    /// uploaded, which only a reload does (see [`Rebuild::Asset`]); the
    /// same for a mesh, texture or `SurfaceAppearance` set the scene never
    /// downloaded. `unions` is every boolean this place has already carved,
    /// which is what lets a union be re-derived here without re-running one.
    /// Nothing is refused for being *new*: a shape kind the place never
    /// used, a part with no counterpart in the scene, a mesh another
    /// instance already draws through are all patched.
    pub(crate) fn resync_part(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
        known_layers: usize,
        unions: &UnionEvaluations,
    ) -> Result<PartSync, Rebuild> {
        let present = dom.get(referent).is_some()
            && is_drawable(dom, database, referent)
            && in_workspace(dom, database, referent);
        let built = present
            .then(|| build_part(dom, database, referent, &mut self.materials))
            .flatten();
        let Some(mut part) = built else {
            return Ok(PartSync::gone(self.remove_part(referent)));
        };
        if part.material.layer as usize >= known_layers {
            return Err(Rebuild::Asset);
        }

        // One pass for both: where the referent's own box sits and how many
        // pieces stand under it. A single-instance edit walks the part list
        // once, whatever it turns out to be.
        let (held, held_pieces) = self.standing(referent);
        let drawn = match Replanned::of(dom, database, referent, &mut self.materials) {
            None => {
                self.remove_instance(referent);
                Drawn::Box(part)
            }
            Some(Replanned::Union(entry)) => {
                self.resync_union(&mut part, entry, unions, known_layers)?
            }
            Some(Replanned::Mesh(entry)) => {
                // The box stays, suppressed, exactly as `resolve_file_meshes`
                // leaves it: it is what says the referent draws through the
                // mesh path, and what a decal would have been projected on if
                // the mesh had not taken over.
                part.suppressed = true;
                if entry.is_invisible() {
                    self.remove_instance(referent);
                    Drawn::Gone
                } else {
                    let instance = entry
                        .patched(&self.resolved_file_meshes)
                        .ok_or(Rebuild::Asset)?;
                    if instance.material.layer as usize >= known_layers {
                        return Err(Rebuild::Asset);
                    }
                    Drawn::Mesh(self.place_instance(instance))
                }
            }
        };

        let kept = match &drawn {
            Drawn::Pieces { pieces, .. } => pieces.len() as u32,
            _ => 0,
        };
        // Dropping a piece shifts every part after it along, so the box's own
        // slot has to be found again — and only then, which is next to never.
        let held = if kept < held_pieces {
            self.drop_pieces_from(referent, kept);
            self.standing(referent).0
        } else {
            held
        };
        match held {
            Some(index) => self.parts[index] = part,
            None => self.parts.push(part),
        }
        Ok(PartSync {
            drawn,
            dropped: kept.min(held_pieces)..held_pieces,
        })
    }

    /// Takes every box and resolved instance standing for `referent` out of
    /// the scene — all of them, so a deleted union goes with its pieces —
    /// and names the piece slots the renderer must now let go of too.
    pub(crate) fn remove_part(&mut self, referent: Ref) -> Range<u32> {
        let dropped = 0..self.piece_count(referent);
        self.parts.retain(|part| part.referent() != referent);
        self.remove_instance(referent);
        dropped
    }

    /// Where `referent`'s own box sits in `parts`, and how many recovered
    /// pieces stand under it.
    fn standing(&self, referent: Ref) -> (Option<usize>, u32) {
        let mut whole = None;
        let mut pieces = 0;
        for (index, part) in self.parts.iter().enumerate() {
            if part.referent() != referent {
                continue;
            }
            match part.id.is_whole() {
                true => whole = Some(index),
                false => pieces += 1,
            }
        }
        (whole, pieces)
    }

    /// How many recovered pieces this scene draws `referent` as — zero for
    /// everything but a union whose boolean failed.
    pub(crate) fn piece_count(&self, referent: Ref) -> u32 {
        self.standing(referent).1
    }

    /// Takes `referent`'s recovered pieces from `first` on out of the scene,
    /// leaving its own box and the pieces before `first` alone.
    fn drop_pieces_from(&mut self, referent: Ref, first: u32) {
        self.parts.retain(|part| {
            part.referent() != referent || part.id.piece_index().is_none_or(|index| index < first)
        });
    }

    fn remove_instance(&mut self, referent: Ref) {
        // Order only ever mattered to a full build's batch construction; the
        // renderer finds instances by referent from here on.
        let instances = &mut self.resolved_file_meshes.instances;
        if let Some(index) = instances
            .iter()
            .position(|instance| instance.referent == referent)
        {
            instances.swap_remove(index);
        }
    }

    /// Puts one re-derived mesh instance where the scene already held that
    /// referent's, or at the end, and says where it landed.
    fn place_instance(&mut self, instance: super::ResolvedInstance) -> usize {
        let instances = &mut self.resolved_file_meshes.instances;
        match instances
            .iter()
            .position(|held| held.referent == instance.referent)
        {
            Some(index) => {
                instances[index] = instance;
                index
            }
            None => {
                instances.push(instance);
                instances.len() - 1
            }
        }
    }

    /// Where `referent`'s box is drawn, if this scene draws one for it — the
    /// entry [`Scene::placements`] would hold for it, without building the
    /// whole map to look up one part. A union drawn as its recovered pieces
    /// keeps its own box's placement, for the same reason `placements` does.
    pub(crate) fn placement_of(&self, referent: Ref) -> Option<Placement> {
        let mut whole = None;
        let mut pieced = false;
        for part in self.parts.iter().filter(|part| part.referent() == referent) {
            match part.id.is_whole() {
                true => whole = Some(part),
                false => pieced = true,
            }
        }
        whole
            .filter(|part| !part.suppressed || pieced)
            .map(|part| part.placement())
    }

    /// Recomputes the scene's extent after its parts changed, reporting
    /// whether it moved. The same box [`Scene::from_dom`] computes: every
    /// instance's own box as the DOM lists it, suppressed or not, but not a
    /// failed union's recovered pieces, which `from_dom` never saw either —
    /// it took its bounds before `resolve_unions` appended them. A scene left
    /// with no part at all keeps its last extent rather than none: the camera
    /// and the shadow fit still need a box to work against.
    pub(crate) fn refresh_bounds(&mut self) -> bool {
        let originals: Vec<Part> = self
            .parts
            .iter()
            .filter(|part| part.id.is_whole())
            .copied()
            .collect();
        match bounds::of(&originals) {
            Some(extent) if extent != self.bounds => {
                self.bounds = extent;
                true
            }
            _ => false,
        }
    }

    /// Re-reads every enabled `ScreenGui` from `dom`, the way
    /// [`Scene::from_dom`] did.
    pub(crate) fn replan_gui_screens(&mut self, dom: &WeakDom, database: &ReflectionDatabase) {
        self.gui = super::gui::plan(dom, database);
    }

    /// Re-reads every placeable `BillboardGui`/`SurfaceGui` from `dom`,
    /// measured against the parts as this scene now draws them — the way
    /// [`Scene::from_dom`] did. The whole list, for the same reason
    /// [`Scene::replan_effect`] re-reads a whole effect list: paint order is
    /// tree order, and a canvas re-placed on its own would lose its place in
    /// it.
    pub(crate) fn replan_gui_spaces(&mut self, dom: &WeakDom, database: &ReflectionDatabase) {
        let placements = self.placements();
        self.gui_spaces = super::gui::plan_space(dom, database, &placements);
    }

    /// Whether any `BillboardGui`/`SurfaceGui` canvas this scene placed hangs
    /// off `referent` — through `Adornee`, which can name a part anywhere,
    /// not only the container's parent.
    pub(crate) fn adorns(&self, referent: Ref) -> bool {
        self.gui_spaces.iter().any(|gui| gui.adornee == referent)
    }
}

/// Whether `referent` hangs under the DOM's `Workspace` service — the only
/// subtree the scene draws from (see [`super::workspace_descendants`]).
/// Walks up the parent chain rather than down from `Workspace`, so it costs
/// the instance's depth instead of the size of the place.
pub(crate) fn in_workspace(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    let mut current = referent;
    while let Some(parent) = dom.parent(current) {
        current = parent;
    }
    current != referent
        && dom
            .get(current)
            .is_some_and(|root| database.is_subclass_of(root.class(), WORKSPACE_CLASS))
}

#[cfg(test)]
#[path = "resync/tests.rs"]
mod tests;
