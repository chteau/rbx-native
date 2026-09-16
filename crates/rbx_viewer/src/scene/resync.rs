//! Bringing one `BasePart` of a built scene in line with the DOM — added,
//! edited, moved or gone — see [`Scene::resync_part`].

use std::collections::HashSet;

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
    /// downloaded. A union drawn as its recovered pieces is refused as
    /// [`Rebuild::Union`]. Nothing is refused for being *new*: a shape kind
    /// the place never used, a part with no counterpart in the scene, a
    /// mesh another instance already draws through are all patched.
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

        let mut standing_in = self
            .parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.referent == referent)
            .map(|(index, _)| index);
        let held = standing_in.next();
        if standing_in.next().is_some() {
            // A failed union's recovered pieces all answer to the union's
            // referent (see `union::tree`): no one box recomputed from the
            // union's own properties stands for the lot. Only an edit is
            // refused — a union gone from the DOM went, above, with every
            // piece.
            return Err(Rebuild::Union);
        }
        if part.material.layer as usize >= known_layers {
            return Err(Rebuild::Asset);
        }

        let sync = match Replanned::of(dom, database, referent, &mut self.materials) {
            None => {
                self.remove_instance(referent);
                PartSync::Box(part)
            }
            Some(entry) => {
                // The box stays, suppressed, exactly as `resolve_file_meshes`
                // and `resolve_unions` leave it: it is what says the referent
                // draws through the mesh path, and what a decal would have
                // been projected on if the mesh had not taken over.
                part.suppressed = true;
                if entry.is_invisible() {
                    self.remove_instance(referent);
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
                    let instances = &mut self.resolved_file_meshes.instances;
                    let index = match instances.iter().position(|held| held.referent == referent) {
                        Some(index) => {
                            instances[index] = instance;
                            index
                        }
                        None => {
                            instances.push(instance);
                            instances.len() - 1
                        }
                    };
                    PartSync::Mesh(index)
                }
            }
        };

        match held {
            Some(index) => self.parts[index] = part,
            None => self.parts.push(part),
        }
        Ok(sync)
    }

    /// Takes every box and resolved instance standing for `referent` out of
    /// the scene — all of them, so a deleted union goes with its pieces.
    pub(crate) fn remove_part(&mut self, referent: Ref) {
        self.parts.retain(|part| part.referent != referent);
        self.remove_instance(referent);
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

    /// Where `referent`'s box is drawn, if this scene draws it as one — the
    /// entry [`Scene::placements`] would hold for it, without building the
    /// whole map to look up one part.
    pub(crate) fn placement_of(&self, referent: Ref) -> Option<Placement> {
        self.parts
            .iter()
            .find(|part| part.referent == referent && !part.suppressed)
            .map(Part::placement)
    }

    /// Recomputes the scene's extent after its parts changed, reporting
    /// whether it moved. The same box [`Scene::from_dom`] computes: every
    /// part as the DOM lists it, suppressed or not, but not a failed union's
    /// recovered pieces, which `from_dom` never saw either — it took its
    /// bounds before `resolve_unions` appended them. A scene left with no
    /// part at all keeps its last extent rather than none: the camera and
    /// the shadow fit still need a box to work against.
    pub(crate) fn refresh_bounds(&mut self) -> bool {
        let suppressed: HashSet<Ref> = self
            .parts
            .iter()
            .filter(|part| part.suppressed)
            .map(|part| part.referent)
            .collect();
        let originals: Vec<Part> = self
            .parts
            .iter()
            .filter(|part| part.suppressed || !suppressed.contains(&part.referent))
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
