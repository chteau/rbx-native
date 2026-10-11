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

use std::borrow::Cow;
use std::collections::HashSet;

use glam::Vec3;
use rbx_dom::{Ref, WeakDom};

use super::{bounds, Beam, Bounds, Emitter, Part, Scene, SpaceGui, Trail};

/// How far, in studs, the floor an isolated rig stands on reaches from the
/// rig's centre in x and z: the half-width of the Animation Editor's stage
/// grid, and of the patch of ground [`Scene::stage_bounds`] fits the sun's
/// shadow map to so the rig's shadow lands on the whole grid.
pub const STAGE_REACH: f32 = 20.0;

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
    /// The particle emitters, beams, trails and in-world GUIs that belong to
    /// the isolated roots and so still draw — see [`Scene::sweep_effects`].
    effects: HashSet<Ref>,
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
            Isolation {
                roots,
                inside,
                effects: HashSet::new(),
            }
        });
        self.flag_isolated();
        self.sweep_effects(&parent_of);
    }

    /// Re-decides which effects belong to the isolated roots: an effect
    /// draws when it sits under a root itself, when either attachment a
    /// beam or trail spans does, or when the part a `BillboardGui`/
    /// `SurfaceGui` hangs off does. A sword's trail is parented to the
    /// sword but stretched between attachments on it; a nametag lives in
    /// `PlayerGui` but adorns the head — both belong to the rig.
    ///
    /// Run after [`Scene::isolate`] and after every replan of an effect list
    /// (see `Scene::replan_effect`), which is the only time those lists
    /// change.
    pub(crate) fn sweep_effects(&mut self, parent_of: &impl Fn(Ref) -> Option<Ref>) {
        let Some(isolation) = &mut self.isolation else {
            return;
        };
        let roots = &isolation.roots;
        let belongs = |refs: &[Ref]| refs.iter().any(|&at| within(roots, at, parent_of));
        isolation.effects = self
            .emitters
            .iter()
            .map(|emitter| emitter.referent)
            .filter(|&referent| belongs(&[referent]))
            .chain(
                self.beams
                    .iter()
                    .filter(|beam| {
                        belongs(&[beam.referent, beam.attachments[0], beam.attachments[1]])
                    })
                    .map(|beam| beam.referent),
            )
            .chain(
                self.trails
                    .iter()
                    .filter(|trail| {
                        belongs(&[trail.referent, trail.attachments[0], trail.attachments[1]])
                    })
                    .map(|trail| trail.referent),
            )
            .chain(
                self.gui_spaces
                    .iter()
                    .filter(|gui| belongs(&[gui.referent, gui.adornee]))
                    .map(|gui| gui.referent),
            )
            .collect();
    }

    /// What an isolated view's sun shadow is fitted to: the parts still
    /// drawn, joined with a [`STAGE_REACH`] square of floor around their
    /// centre at their lowest point — the stage grid the rig stands on.
    /// `None` while nothing is isolated, or when no part is left drawn; the
    /// renderer then fits to the whole place as usual.
    ///
    /// Without it the shadow map stays spread over the whole place while
    /// only the rig is drawn, and a rig in a large place gets a handful of
    /// texels for its whole shadow.
    pub(crate) fn stage_bounds(&self) -> Option<Bounds> {
        self.isolation.as_ref()?;
        let rig = bounds::of(
            self.parts
                .iter()
                .filter(|part| part.id.is_whole() && !part.isolated_out),
        )?;
        let centre = rig.center();
        let reach = Vec3::new(STAGE_REACH, 0.0, STAGE_REACH);
        let floor = Vec3::new(centre.x, rig.min.y, centre.z);
        Some(rig.union(Bounds {
            min: floor - reach,
            max: floor + reach,
        }))
    }

    /// Whether the effect read from `referent` draws: always while nothing
    /// is isolated, and only for the isolated roots' own effects otherwise.
    fn effect_drawn(&self, referent: Ref) -> bool {
        self.isolation
            .as_ref()
            .is_none_or(|isolation| isolation.effects.contains(&referent))
    }

    /// [`Scene::particle_emitters`] less the ones an isolation hides — what
    /// the renderer builds from. The full list stays for asset collection.
    pub(crate) fn drawn_particle_emitters(&self) -> Cow<'_, [Emitter]> {
        self.drawn(&self.emitters, |emitter| emitter.referent)
    }

    /// [`Scene::beams`] less the ones an isolation hides.
    pub(crate) fn drawn_beams(&self) -> Cow<'_, [Beam]> {
        self.drawn(&self.beams, |beam| beam.referent)
    }

    /// [`Scene::trails`] less the ones an isolation hides.
    pub(crate) fn drawn_trails(&self) -> Cow<'_, [Trail]> {
        self.drawn(&self.trails, |trail| trail.referent)
    }

    /// Every `BillboardGui`/`SurfaceGui` this scene could place, each already
    /// carrying the pixel size of the canvas its tree is painted into, less
    /// the ones an isolation hides. The renderer owns that canvas — a `Scene`
    /// has no GPU handle — and finishes a billboard's quad, which only exists
    /// once an eye does.
    pub(crate) fn drawn_gui_spaces(&self) -> Cow<'_, [SpaceGui]> {
        self.drawn(&self.gui_spaces, |gui| gui.referent)
    }

    fn drawn<'a, T: Clone>(&self, all: &'a [T], referent: impl Fn(&T) -> Ref) -> Cow<'a, [T]> {
        if self.isolation.is_none() {
            return Cow::Borrowed(all);
        }
        Cow::Owned(
            all.iter()
                .filter(|item| self.effect_drawn(referent(item)))
                .cloned()
                .collect(),
        )
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
