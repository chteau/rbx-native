//! What a batch of changes owes at its end, once every instance in it has
//! been seen: the lists that are cheaper to re-plan whole than to patch one
//! member of — each once per batch, however many members changed — and the
//! extent, the lighting and the local lights the same way.

use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::Patcher;
use crate::changes::Rebuild;
use crate::lighting::{self, Lighting};
use crate::scene::{Bounds, EffectKind};

/// What a batch owes at its end, once every instance in it has been seen:
/// each list re-planned once however many of its members changed.
#[derive(Default)]
pub(super) struct Pending {
    /// Any part changed, so the extent may have.
    pub(super) parts: bool,
    pub(super) lighting: bool,
    pub(super) lights: bool,
    /// By `EffectKind` — see [`Pending::effect`].
    pub(super) effects: [bool; 3],
    /// The `ScreenGui` overlays, and the `BillboardGui`/`SurfaceGui`
    /// canvases placed in the scene — two lists, re-planned apart, since a
    /// part that moved can only have carried a canvas.
    pub(super) screens: bool,
    pub(super) spaces: bool,
    /// An `Attachment` moved, came or went. What hangs off one — a `Beam`
    /// or `Trail` end, a `Light` — is re-planned only if the scene has any
    /// of that kind at all (see [`Pending::attachment_due`]): attachments
    /// are everywhere in a real place (every weld, every constraint), and
    /// re-walking the whole DOM for a beam plan that is empty before and
    /// after is what made dragging any model cost a full-place walk per
    /// mouse move.
    pub(super) attachments: bool,
}

impl Pending {
    pub(super) fn effect(&mut self, kind: EffectKind) {
        self.effects[Self::slot(kind)] = true;
    }

    pub(super) fn slot(kind: EffectKind) -> usize {
        match kind {
            EffectKind::Particles => 0,
            EffectKind::Beams => 1,
            EffectKind::Trails => 2,
        }
    }

    /// An `Attachment` moved, came or went: it can be the endpoint of any
    /// number of beams and trails, and the anchor of a light.
    pub(super) fn attachment(&mut self) {
        self.attachments = true;
    }

    /// Whether a plan that attachments feed (a beam, a trail, the local
    /// lights) is due: asked for outright, or an attachment moved while the
    /// scene actually holds some of that kind (`any_now`).
    pub(super) fn attachment_due(&self, asked: bool, any_now: bool) -> bool {
        asked || (self.attachments && any_now)
    }
}

impl Patcher<'_> {
    /// Which GUI list a changed instance belongs to, from the container it
    /// sits in — `start` being the instance itself, or the parent it left
    /// once it is gone. A `ScreenGui` is an overlay, a `BillboardGui`/
    /// `SurfaceGui` a canvas, and a `Frame` whichever holds it; with no
    /// container to be found (the parent went too), both lists are owed.
    pub(super) fn gui_changed(&mut self, start: Option<Ref>) {
        let dom = self.dom;
        let mut current = start;
        while let Some(referent) = current {
            let Some(instance) = dom.get(referent) else {
                break;
            };
            if self.database.is_subclass_of(instance.class(), "ScreenGui") {
                self.pending.screens = true;
                return;
            }
            if self
                .database
                .is_subclass_of(instance.class(), "BillboardGui")
                || self.database.is_subclass_of(instance.class(), "SurfaceGui")
            {
                self.pending.spaces = true;
                return;
            }
            current = dom.parent(referent);
        }
        self.pending.screens = true;
        self.pending.spaces = true;
    }

    pub(super) fn finish(&mut self) -> Result<Option<Bounds>, Rebuild> {
        let dom = self.dom;
        let database = self.database;
        let mut moved = None;
        if self.pending.parts && self.loaded.scene_mut().refresh_bounds() {
            let bounds = *self.loaded.scene().bounds();
            self.offscreen
                .with_renderer(|renderer, _, _| renderer.set_bounds(bounds));
            moved = Some(bounds);
        }
        if self.pending.lighting {
            let lighting = Lighting::from_dom(dom, database, self.toggles.clock_time);
            self.loaded.set_lighting(lighting);
            self.offscreen
                .with_renderer(|renderer, _, _| renderer.set_lighting(lighting));
        }
        if self
            .pending
            .attachment_due(self.pending.lights, !self.loaded.lights().is_empty())
        {
            // The same list a full build collects, off the same centre —
            // which only matters past `MAX_LOCAL_LIGHTS`, where it decides
            // which lights are kept.
            let lights = if self.toggles.lights {
                lighting::local_lights(dom, database, self.loaded.scene().bounds().center())
            } else {
                Vec::new()
            };
            if lights != self.loaded.lights() {
                self.offscreen.with_renderer(|renderer, device, queue| {
                    renderer.set_lights(device, queue, &lights)
                });
                self.loaded.set_lights(lights);
            }
        }
        for kind in [EffectKind::Particles, EffectKind::Beams, EffectKind::Trails] {
            let any_now = match kind {
                EffectKind::Particles => false,
                EffectKind::Beams => !self.loaded.scene().beams().is_empty(),
                EffectKind::Trails => !self.loaded.scene().trails().is_empty(),
            };
            if !self
                .pending
                .attachment_due(self.pending.effects[Pending::slot(kind)], any_now)
            {
                continue;
            }
            self.loaded.scene_mut().replan_effect(dom, database, kind);
            let scene = self.loaded.scene();
            // Always serves the edit now: a texture this renderer has no
            // upload for draws that effect's own fallback until it lands
            // (see `Renderer::patch_effect`) — never a rebuild.
            self.offscreen
                .with_renderer(|renderer, _, _| renderer.patch_effect(kind, scene));
            self.request_effect_textures(kind);
        }
        if self.pending.screens {
            self.loaded.scene_mut().replan_gui_screens(dom, database);
        }
        if self.pending.spaces {
            self.loaded.scene_mut().replan_gui_spaces(dom, database);
        }
        if self.pending.screens || self.pending.spaces {
            // A text object the edit gave a family this session has never
            // seen draws in the fallback face until the family lands; asking
            // now is what makes it land.
            self.loaded.resolve_fonts(self.resident);
            let world = self.loaded.world();
            self.offscreen.with_renderer(|renderer, device, queue| {
                renderer.refresh_gui(device, queue, world)
            });
        }
        Ok(moved)
    }

    /// Asks the background loader for the textures `kind`'s freshly
    /// re-planned list names, whichever of them this session does not have
    /// — the whole list rather than just the touched instance, since
    /// `replan_effect` rebuilds it whole.
    fn request_effect_textures(&mut self, kind: EffectKind) {
        let images: Vec<AssetRef> = {
            let scene = self.loaded.scene();
            let refs: Box<dyn Iterator<Item = &AssetRef>> = match kind {
                EffectKind::Particles => Box::new(
                    scene
                        .particle_emitters()
                        .iter()
                        .map(|emitter| &emitter.texture),
                ),
                EffectKind::Beams => Box::new(scene.beams().iter().map(|beam| &beam.texture)),
                EffectKind::Trails => Box::new(scene.trails().iter().map(|trail| &trail.texture)),
            };
            let mut wanted = Vec::new();
            for reference in refs {
                if *reference != AssetRef::Empty && !wanted.contains(reference) {
                    wanted.push(reference.clone());
                }
            }
            wanted
        };
        if !images.is_empty() {
            self.loaded.also_wants(&images);
            self.resident.images(&images);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attachment_move_replans_only_what_the_scene_already_has() {
        let mut pending = Pending::default();
        pending.attachment();
        assert!(pending.attachment_due(false, true), "a beam exists to move");
        assert!(
            !pending.attachment_due(false, false),
            "no beam, nothing to re-plan"
        );
        assert!(
            pending.attachment_due(true, false),
            "asked for outright, planned regardless"
        );
        assert!(
            !Pending::default().attachment_due(false, true),
            "nothing moved at all"
        );
    }
}
