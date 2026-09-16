//! Bringing one `BasePart` of a built scene in line with the DOM — added,
//! edited, moved or gone — see [`Scene::resync_part`].

mod pieces;

use std::ops::Range;

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::patch::{Assets, Replanned};
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

/// Where a referent stands in `Scene::parts`, so an edit finds it without a
/// pass over the place — see [`Scene::standing_of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Standing {
    /// Its own box. `None` only while a union being inserted has had its
    /// recovered pieces placed and its own box not yet.
    pub(super) whole: Option<usize>,
    /// How many of those pieces stand under it (see `union::tree`) — zero
    /// for everything but a union whose boolean failed. They are counted
    /// rather than indexed one by one: a handful of parts under one
    /// referent, and only an edit of that very union ever walks them.
    pub(super) pieces: u32,
}

impl Standing {
    /// A referent's own box, standing alone at `index`.
    pub(super) fn whole(index: usize) -> Self {
        Standing {
            whole: Some(index),
            pieces: 0,
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
    /// uploaded, which only a reload does (see [`Rebuild::Asset`]).
    ///
    /// A mesh, texture, `SurfaceAppearance` set or union boolean this
    /// session has not resolved yet is never a rebuild either: the part
    /// draws as its box, exactly as a full build with that asset withheld
    /// leaves it, the asset is asked for in the background (see
    /// `Headless::apply_changes`), and the box gives way to the resolved
    /// instance — or, for a union whose boolean failed, its recovered
    /// pieces — in place whenever it lands; otherwise every later edit of a
    /// `MeshPart`/union whose asset had not arrived yet would be a reload
    /// for the rest of the session. `unions` is every boolean this place
    /// has already carved, which is what lets a union be re-derived here
    /// without re-running one (see [`Scene::resync_union`]). Nothing is
    /// refused for being *new*: a shape kind the place never used, a part
    /// with no counterpart in the scene, a mesh another instance already
    /// draws through are all patched.
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

        // Where the referent's own box sits and how many pieces stand under
        // it, read before anything below moves either.
        let (held, held_pieces) = self.standing_of(referent);
        let drawn = match Replanned::of(dom, database, referent, &mut self.materials) {
            None => {
                self.file_mesh_plan.install(referent, None);
                self.resolved_file_meshes.remove(referent);
                Drawn::Box(part)
            }
            Some(Replanned::Union(entry)) => {
                self.resync_union(&mut part, entry, unions, known_layers)?
            }
            Some(Replanned::Mesh(mesh_entry)) => {
                // A single-instance edit that changes what this referent
                // draws through has to leave the file-mesh plan saying so,
                // or a mesh landing later would resolve against the
                // `MeshId` the place opened with (see
                // `filemesh::Plan::install`).
                self.file_mesh_plan
                    .install(referent, Some(mesh_entry.clone()));
                if !self
                    .resolved_file_meshes
                    .meshes
                    .contains_key(mesh_entry.asset())
                {
                    // Before the transparency check on purpose: a fully
                    // transparent box keeps its placement, and a full
                    // build never hid the box of a part whose mesh did
                    // not come.
                    //
                    // Never a rebuild for this reason: the missing asset
                    // is asked for in the background (see
                    // `Headless::apply_changes`) and folded in whenever it
                    // lands.
                    self.resolved_file_meshes.remove(referent);
                    Drawn::Box(part)
                } else {
                    // The box stays, suppressed, exactly as
                    // `resolve_file_meshes` leaves it: it is what says the
                    // referent draws through the mesh path, and what a
                    // decal would have been projected on if the mesh had
                    // not taken over.
                    part.suppressed = true;
                    if mesh_entry.is_invisible() {
                        self.resolved_file_meshes.remove(referent);
                        Drawn::Gone
                    } else {
                        match mesh_entry.patched(&self.resolved_file_meshes) {
                            // A texture, `SurfaceAppearance` set or
                            // material sample not finished downloading:
                            // the same "not landed yet" case as the mesh
                            // itself not resolving, so the box until it
                            // does.
                            None => {
                                part.suppressed = false;
                                self.resolved_file_meshes.remove(referent);
                                Drawn::Box(part)
                            }
                            Some(instance) if instance.material.layer as usize >= known_layers => {
                                return Err(Rebuild::Asset);
                            }
                            Some(instance) => Drawn::Mesh(self.place_instance(instance)),
                        }
                    }
                }
            }
        };

        let kept = match &drawn {
            Drawn::Pieces { pieces, .. } => pieces.len() as u32,
            _ => 0,
        };
        // Dropping a piece moves whichever part fills its slot, so the box's
        // own slot has to be read again — and only then, which is next to
        // never.
        let held = if kept < held_pieces {
            self.drop_pieces_from(referent, kept);
            self.standing_of(referent).0
        } else {
            held
        };
        let old = held.map(|index| self.parts[index]);
        match held {
            Some(index) => self.parts[index] = part,
            None => self.push_part(part),
        }
        self.note_extent(old.as_ref(), Some(&part));
        Ok(PartSync {
            drawn,
            dropped: kept.min(held_pieces)..held_pieces,
        })
    }

    /// The mesh, union and image assets a fresh re-plan of `referent` would
    /// need, whether or not this session already has them — what
    /// `Headless::apply_changes` asks the background loader for right after
    /// [`Scene::resync_part`], so an asset that just took the box-fallback
    /// path is asked for rather than left missing for good. Empty for a
    /// referent [`Replanned::of`] has nothing to say about (a plain `Part`),
    /// but every referent still in `dom` also gets its own material sample's
    /// maps checked, whether or not it is mesh/union-backed: a brand-new
    /// material named for the first time needs its own pack fetched too.
    pub(crate) fn wanted_assets_of(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
    ) -> Assets {
        let mut wanted = Replanned::of(dom, database, referent, &mut self.materials)
            .map(|entry| entry.assets())
            .unwrap_or_default();
        if let Some(instance) = dom.get(referent) {
            let slot = self.materials.slot_for(instance.properties(), database);
            wanted.images.extend(self.materials.maps_of(slot.layer));
        }
        wanted
    }

    /// Takes every box and resolved instance standing for `referent` out of
    /// the scene — all of them, so a deleted union goes with its pieces —
    /// and names the piece slots the renderer must now let go of too.
    pub(crate) fn remove_part(&mut self, referent: Ref) -> Range<u32> {
        self.resolved_file_meshes.remove(referent);
        let Some(standing) = self.standing.remove(&referent) else {
            return 0..0;
        };
        if let Some(index) = standing.whole {
            let whole = self.parts[index];
            self.note_extent(Some(&whole), None);
        }
        let doomed = match standing.pieces {
            // The one pass that still walks the parts: pieces are counted,
            // not indexed, and a failed union being deleted is rare enough
            // not to be worth indexing them for.
            1.. => self.slots_of(referent, |_| true),
            0 => standing.whole.into_iter().collect(),
        };
        self.remove_slots(doomed);
        0..standing.pieces
    }

    /// Where `referent`'s own box sits in `parts`, and how many recovered
    /// pieces stand under it.
    fn standing_of(&self, referent: Ref) -> (Option<usize>, u32) {
        match self.standing.get(&referent) {
            Some(standing) => (standing.whole, standing.pieces),
            None => (None, 0),
        }
    }

    /// How many recovered pieces this scene draws `referent` as — zero for
    /// everything but a union whose boolean failed.
    pub(crate) fn piece_count(&self, referent: Ref) -> u32 {
        self.standing_of(referent).1
    }

    /// Takes `referent`'s recovered pieces from `first` on out of the scene,
    /// leaving its own box and the pieces before `first` alone.
    fn drop_pieces_from(&mut self, referent: Ref, first: u32) {
        let doomed = self.slots_of(referent, |part| {
            part.id.piece_index().is_some_and(|index| index >= first)
        });
        self.remove_slots(doomed);
    }

    /// Every slot `referent` fills that `wanted` accepts.
    fn slots_of(&self, referent: Ref, wanted: impl Fn(&Part) -> bool) -> Vec<usize> {
        self.parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.referent() == referent && wanted(part))
            .map(|(index, _)| index)
            .collect()
    }

    /// Takes the parts at `doomed` out, highest slot first: [`remove_slot`]
    /// fills each hole with the last part, which would renumber a slot still
    /// to be taken out if it ran the other way round.
    ///
    /// [`remove_slot`]: Scene::remove_slot
    fn remove_slots(&mut self, mut doomed: Vec<usize>) {
        doomed.sort_unstable();
        for index in doomed.into_iter().rev() {
            self.remove_slot(index);
        }
    }

    /// Takes the part at `index` out, the last one filling the hole and told
    /// where it now stands. Order among parts only ever mattered to a full
    /// build's batch construction; a patch finds parts by referent.
    fn remove_slot(&mut self, index: usize) {
        let dropped = self.parts.swap_remove(index);
        if !dropped.id.is_whole() {
            if let Some(standing) = self.standing.get_mut(&dropped.referent()) {
                standing.pieces -= 1;
            }
        }
        let Some(moved) = self.parts.get(index) else {
            return;
        };
        if moved.id.is_whole() {
            if let Some(standing) = self.standing.get_mut(&moved.referent()) {
                standing.whole = Some(index);
            }
        }
    }

    /// Puts one re-derived mesh instance where the scene already held that
    /// referent's, or at the end, and says where it landed.
    fn place_instance(&mut self, instance: super::ResolvedInstance) -> usize {
        let resolved = &mut self.resolved_file_meshes;
        match resolved.slot_of(instance.referent) {
            Some(index) => {
                resolved.instances[index] = instance;
                index
            }
            None => resolved.push(instance),
        }
    }

    /// Where `referent`'s box is drawn, if this scene draws one for it — the
    /// entry [`Scene::placements`] would hold for it, without building the
    /// whole map to look up one part. A union drawn as its recovered pieces
    /// keeps its own box's placement, for the same reason `placements` does.
    pub(crate) fn placement_of(&self, referent: Ref) -> Option<Placement> {
        let standing = self.standing.get(&referent)?;
        let part = self.parts.get(standing.whole?)?;
        (!part.suppressed || standing.pieces > 0).then(|| part.placement())
    }

    /// Keeps the extent in step with one part going from `old` to `new`
    /// (either absent): grown on the spot by the new corners, which is all a
    /// move outward or an insert needs, and marked for a recount only when
    /// the old part may have been holding an edge — nothing short of every
    /// part says where that edge is now. A part strictly inside the box
    /// therefore moves for free, however many parts the place has.
    fn note_extent(&mut self, old: Option<&Part>, new: Option<&Part>) {
        if let Some(old) = old.filter(|old| counts_towards_extent(old)) {
            let was = bounds::of_part(old);
            if was.min.cmple(self.bounds.min).any() || was.max.cmpge(self.bounds.max).any() {
                self.extent_stale = true;
            }
        }
        if let Some(new) = new.filter(|new| counts_towards_extent(new)) {
            let now = bounds::of_part(new);
            self.bounds.min = self.bounds.min.min(now.min);
            self.bounds.max = self.bounds.max.max(now.max);
        }
    }

    /// The scene's extent after its parts changed, and whether it moved
    /// since the last call. Exact either way: what `note_extent` grew is the
    /// answer unless a part that may have held an edge changed, in which
    /// case every part is counted again. A scene left with no part at all
    /// keeps its last extent rather than none: the camera and the shadow fit
    /// still need a box to work against.
    pub(crate) fn refresh_bounds(&mut self) -> bool {
        if std::mem::take(&mut self.extent_stale) {
            let originals = self.parts.iter().filter(|part| counts_towards_extent(part));
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

/// The same box [`Scene::from_dom`] computes: every instance's own box as
/// the DOM lists it, suppressed or not, but not a failed union's recovered
/// pieces, which `from_dom` never saw either — it took its bounds before
/// `resolve_unions` appended them.
fn counts_towards_extent(part: &Part) -> bool {
    part.id.is_whole()
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
