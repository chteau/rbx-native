//! Bringing one `BasePart` of a built scene in line with the DOM — added,
//! edited, moved or gone — see [`Scene::resync_part`].

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::patch::Replanned;
use super::{bounds, build_part, is_drawable, Part, Placement, Scene, WORKSPACE_CLASS};
use crate::changes::Rebuild;

/// What the renderer now has to show for one referent after
/// [`Scene::resync_part`] — every pass either rewrites its record for it or
/// takes it out, whichever the variant says.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PartSync {
    /// Drawn as its unit-shape box, `Part::is_drawn` permitting: a fully
    /// transparent box still keeps its placement, for the decals on it.
    Box(Part),
    /// Drawn as a resolved file mesh or union: `Scene::resolved_file_meshes`'s
    /// `instances[index]`. Its box is suppressed, and with it its placement.
    Mesh(usize),
    /// Nothing drawn for it any more: not in the DOM, not under `Workspace`,
    /// not a drawable class, without a size or a frame, or a mesh part that
    /// turned invisible.
    Gone,
}

/// Where a referent stands in `Scene::parts` — see `Scene::standing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Standing {
    /// Its one box, at this index.
    Box(usize),
    /// A failed union's recovered pieces, which all answer to the union's
    /// own referent alongside its suppressed box (see `union::tree`): no
    /// single slot stands for the lot, and an edit of one is a rebuild.
    Pieces,
}

impl Scene {
    /// Recomputes one part from `dom`, exactly as [`Scene::from_dom`] and
    /// the resolutions after it would build it, and puts the result in
    /// place of whatever this scene held for `referent` — which may be
    /// nothing at all (an insert, a `Model` moved into `Workspace`), a box,
    /// or a resolved mesh, and may become any of the three.
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
    /// asked for. A mesh or union asset it asked for and never got (see
    /// [`Scene::unresolved`]) is not one of those: the part draws as its
    /// box, exactly as the full build left it, and the edit is a box edit —
    /// otherwise every later edit of a `MeshPart` whose mesh once failed to
    /// download would be a reload, for the rest of the session. A union
    /// drawn as its recovered pieces is refused as [`Rebuild::Union`].
    /// Nothing is refused for being *new*: a shape kind the place never
    /// used, a part with no counterpart in the scene, a mesh another
    /// instance already draws through are all patched.
    pub(crate) fn resync_part(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
        known_layers: usize,
    ) -> Result<PartSync, Rebuild> {
        let present = dom.get(referent).is_some()
            && is_drawable(dom, database, referent)
            && in_workspace(dom, database, referent);
        let built = present
            .then(|| build_part(dom, database, referent, &mut self.materials))
            .flatten();
        let Some(mut part) = built else {
            self.remove_part(referent);
            return Ok(PartSync::Gone);
        };

        // Only an edit is refused for being a union's pieces — a union gone
        // from the DOM went, above, with every piece.
        let held = match self.standing.get(&referent) {
            Some(Standing::Box(index)) => Some(*index),
            Some(Standing::Pieces) => return Err(Rebuild::Union),
            None => None,
        };
        if part.material.layer as usize >= known_layers {
            return Err(Rebuild::Asset);
        }

        let sync = match Replanned::of(dom, database, referent, &mut self.materials) {
            None => {
                self.resolved_file_meshes.remove(referent);
                PartSync::Box(part)
            }
            Some(entry) if !self.resolved_file_meshes.meshes.contains_key(entry.asset()) => {
                // Before the transparency check on purpose: a fully
                // transparent box keeps its placement, and a full build
                // never hid the box of a part whose mesh did not come.
                if !self.unresolved.contains(entry.asset()) {
                    return Err(Rebuild::Asset);
                }
                self.resolved_file_meshes.remove(referent);
                PartSync::Box(part)
            }
            Some(entry) => {
                // The box stays, suppressed, exactly as `resolve_file_meshes`
                // and `resolve_unions` leave it: it is what says the referent
                // draws through the mesh path, and what a decal would have
                // been projected on if the mesh had not taken over.
                part.suppressed = true;
                if entry.is_invisible() {
                    self.resolved_file_meshes.remove(referent);
                    PartSync::Gone
                } else {
                    let missing = match entry {
                        Replanned::Mesh(_) => Rebuild::Asset,
                        Replanned::Union(_) => Rebuild::Union,
                    };
                    let instance = entry.patched(&self.resolved_file_meshes).ok_or(missing)?;
                    if instance.material.layer as usize >= known_layers {
                        return Err(Rebuild::Asset);
                    }
                    let resolved = &mut self.resolved_file_meshes;
                    let index = match resolved.slot_of(referent) {
                        Some(index) => {
                            resolved.instances[index] = instance;
                            index
                        }
                        None => resolved.push(instance),
                    };
                    PartSync::Mesh(index)
                }
            }
        };

        let old = held.map(|index| self.parts[index]);
        match held {
            Some(index) => self.parts[index] = part,
            None => self.push_part(part),
        }
        self.note_extent(old.as_ref(), Some(&part));
        Ok(sync)
    }

    /// Takes every box and resolved instance standing for `referent` out of
    /// the scene — all of them, so a deleted union goes with its pieces.
    pub(crate) fn remove_part(&mut self, referent: Ref) {
        match self.standing.get(&referent).copied() {
            Some(Standing::Box(index)) => {
                let old = self.parts[index];
                self.note_extent(Some(&old), None);
                self.standing.remove(&referent);
                self.remove_slot(index);
            }
            // The one case that still scans: pieces are not indexed, and a
            // failed union being deleted is rare enough not to be.
            Some(Standing::Pieces) => {
                self.extent_stale = true;
                self.standing.remove(&referent);
                while let Some(index) = self.parts.iter().position(|part| part.referent == referent)
                {
                    self.remove_slot(index);
                }
            }
            None => {}
        }
        self.resolved_file_meshes.remove(referent);
    }

    /// Takes the part at `index` out, the last one filling the hole and told
    /// where it now stands. Order among parts only ever mattered to a full
    /// build's batch construction; a patch finds parts by referent.
    fn remove_slot(&mut self, index: usize) {
        self.parts.swap_remove(index);
        if let Some(moved) = self.parts.get(index) {
            if let Some(Standing::Box(slot)) = self.standing.get_mut(&moved.referent) {
                *slot = index;
            }
        }
    }

    /// Where `referent`'s box is drawn, if this scene draws it as one — the
    /// entry [`Scene::placements`] would hold for it, without building the
    /// whole map to look up one part.
    pub(crate) fn placement_of(&self, referent: Ref) -> Option<Placement> {
        let part = match self.standing.get(&referent)? {
            Standing::Box(index) => &self.parts[*index],
            // The first drawn piece, as `placements` would list it.
            Standing::Pieces => self
                .parts
                .iter()
                .find(|part| part.referent == referent && !part.suppressed)?,
        };
        (!part.suppressed).then(|| part.placement())
    }

    /// Keeps the extent in step with one part going from `old` to `new`
    /// (either absent): grown on the spot by the new corners, which is all a
    /// move outward or an insert needs, and marked for a recount only when
    /// the old part may have been holding an edge — nothing short of every
    /// part says where that edge is now. A part strictly inside the box
    /// therefore moves for free, however many parts the place has.
    fn note_extent(&mut self, old: Option<&Part>, new: Option<&Part>) {
        if let Some(old) = old.filter(|old| self.counts_towards_extent(old)) {
            let was = bounds::of_part(old);
            if was.min.cmple(self.bounds.min).any() || was.max.cmpge(self.bounds.max).any() {
                self.extent_stale = true;
            }
        }
        if let Some(new) = new.filter(|new| self.counts_towards_extent(new)) {
            let now = bounds::of_part(new);
            self.bounds.min = self.bounds.min.min(now.min);
            self.bounds.max = self.bounds.max.max(now.max);
        }
    }

    /// The same box [`Scene::from_dom`] computes: every part as the DOM
    /// lists it, suppressed or not, but not a failed union's recovered
    /// pieces, which `from_dom` never saw either — it took its bounds before
    /// `resolve_unions` appended them.
    fn counts_towards_extent(&self, part: &Part) -> bool {
        part.suppressed || matches!(self.standing.get(&part.referent), Some(Standing::Box(_)))
    }

    /// The scene's extent after its parts changed, and whether it moved
    /// since the last call. Exact either way: what `note_extent` grew is the
    /// answer unless a part that may have held an edge changed, in which
    /// case every part is counted again. A scene left with no part at all
    /// keeps its last extent rather than none: the camera and the shadow fit
    /// still need a box to work against.
    pub(crate) fn refresh_bounds(&mut self) -> bool {
        if std::mem::take(&mut self.extent_stale) {
            let originals = self
                .parts
                .iter()
                .filter(|part| self.counts_towards_extent(part));
            if let Some(extent) = bounds::of(originals) {
                self.bounds = extent;
            }
        }
        if self.bounds != self.reported {
            self.reported = self.bounds;
            true
        } else {
            false
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
