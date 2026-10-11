//! Isolation: a scene that draws only what hangs under a few roots — the
//! Animation Editor's one rig on a plain backdrop, the way Blender's
//! animation workspace shows a single armature — while every instance keeps
//! its `Ref`, its slot and its GPU record.
//!
//! Nothing is taken out of the scene for it. A part outside the roots is
//! flagged (see [`Part::is_drawn`]) rather than dropped, which is what lets
//! selection, picking and every edit keep working on the real place, and
//! what makes switching isolation off an exact restore: the flags go back to
//! false and the very same scene draws everything again. Only the shaded
//! passes read the flag; the selection outline, the gizmo and the hover cue
//! stand on [`Scene::all_placements`], which ignores it.

use std::collections::HashSet;

use rbx_dom::{Ref, WeakDom};

use super::{Part, Scene};

/// Which roots a scene is isolated to, and which of its referents were last
/// found under one of them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Isolation {
    roots: HashSet<Ref>,
    /// Kept rather than re-derived from the roots on every sweep: an asset
    /// landing re-resolves the file meshes (see [`Scene::resolve_file_meshes`])
    /// with no DOM in hand to walk, and the sweep after it still has to know
    /// which of the fresh instances stand under a root.
    inside: HashSet<Ref>,
}

impl Isolation {
    pub(crate) fn roots(&self) -> &HashSet<Ref> {
        &self.roots
    }
}

/// Whether `referent` is one of `roots` or sits anywhere beneath one,
/// following `parent_of` up the tree. Walks up rather than down from the
/// roots, so it costs the instance's depth and not the size of the rig.
pub(crate) fn within(
    roots: &HashSet<Ref>,
    referent: Ref,
    parent_of: impl Fn(Ref) -> Option<Ref>,
) -> bool {
    let mut current = Some(referent);
    while let Some(at) = current {
        if roots.contains(&at) {
            return true;
        }
        current = parent_of(at);
    }
    false
}

impl Scene {
    /// Draws only the parts and meshes under `roots` (and the roots
    /// themselves) from now on, or everything again with `None`.
    ///
    /// `parent_of` answers the tree as it stands: a caller with the DOM
    /// passes `WeakDom::parent`, and `Headless`, which keeps no DOM, its
    /// own parent map (see `changes::Roles`). Each referent the scene draws
    /// is asked once, here; later edits keep the answer in step one part at
    /// a time (see [`Scene::resync_part`]).
    pub(crate) fn isolate(
        &mut self,
        roots: Option<&[Ref]>,
        parent_of: impl Fn(Ref) -> Option<Ref>,
    ) {
        self.isolation = roots.map(|roots| {
            let roots: HashSet<Ref> = roots.iter().copied().collect();
            let inside = self
                .parts
                .iter()
                .map(Part::referent)
                .chain(
                    self.resolved_file_meshes
                        .instances
                        .iter()
                        .map(|instance| instance.referent),
                )
                .filter(|&referent| within(&roots, referent, &parent_of))
                .collect();
            Isolation { roots, inside }
        });
        self.flag_isolated();
    }

    /// The roots [`Scene::isolate`] last set, or `None` while the whole
    /// place draws.
    pub(crate) fn isolation(&self) -> Option<&Isolation> {
        self.isolation.as_ref()
    }

    /// Whether `referent` is to be hidden by the isolation as it stands,
    /// asked of `dom` itself and remembered for the next sweep — what a
    /// single-part edit uses, since the part may have just been moved under
    /// a root or out from under one. Always `false` while nothing is
    /// isolated.
    pub(super) fn isolates_out(&mut self, dom: &WeakDom, referent: Ref) -> bool {
        let Some(isolation) = &mut self.isolation else {
            return false;
        };
        let inside = within(&isolation.roots, referent, |at| dom.parent(at));
        match inside {
            true => isolation.inside.insert(referent),
            false => isolation.inside.remove(&referent),
        };
        !inside
    }

    /// Writes the isolation onto every part and resolved instance — after
    /// [`Scene::isolate`], and after anything that rebuilt or appended to
    /// either list without a DOM to ask (a streaming landing, a union's
    /// recovered pieces).
    pub(super) fn flag_isolated(&mut self) {
        let hidden = |referent: Ref| {
            self.isolation
                .as_ref()
                .is_some_and(|isolation| !isolation.inside.contains(&referent))
        };
        for part in &mut self.parts {
            part.isolated_out = hidden(part.referent());
        }
        for instance in &mut self.resolved_file_meshes.instances {
            instance.isolated_out = hidden(instance.referent);
        }
    }
}

#[cfg(test)]
#[path = "isolation/tests.rs"]
mod tests;
