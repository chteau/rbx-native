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
    pub(super) effects: [bool; 5],
    /// The GUI trees (every `ScreenGui`, and every `BillboardGui`/
    /// `SurfaceGui` as an editor's canvas), and the canvases placed in the
    /// scene — two lists, re-planned apart, since a part that moved can
    /// only have carried a canvas.
    pub(super) screens: Screens,
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

/// Which GUI trees a batch re-plans.
///
/// A property written inside one tree — every step of an editor's drag —
/// re-plans that tree alone: the whole list costs a walk of the entire DOM
/// (twice: once for style links, once for roots) and a plan of every tree
/// in the place, which is what made a drag cost the place rather than the
/// edit.
#[derive(Default)]
pub(super) enum Screens {
    #[default]
    None,
    Roots(Vec<Ref>),
    All,
}

impl Screens {
    fn root(&mut self, root: Ref) {
        match self {
            Screens::None => *self = Screens::Roots(vec![root]),
            Screens::Roots(roots) if !roots.contains(&root) => roots.push(root),
            Screens::Roots(_) | Screens::All => {}
        }
    }

    fn owed(&self) -> bool {
        !matches!(self, Screens::None)
    }
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
            EffectKind::Highlights => 3,
            EffectKind::Adornments => 4,
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
    ///
    /// `written` is a property write on a live instance, which re-plans only
    /// the tree it sits in (see [`Screens`]). Anything structural re-plans
    /// every tree, and so does a write to the styling family: a sheet can
    /// sit in one tree and style another through a `StyleLink` anywhere.
    pub(super) fn gui_changed(&mut self, start: Option<Ref>, written: bool) {
        let dom = self.dom;
        let styling = start
            .and_then(|start| dom.get(start))
            .is_some_and(|instance| {
                ["StyleBase", "StyleDerive", "StyleLink"]
                    .iter()
                    .any(|class| self.database.is_subclass_of(instance.class(), class))
            });
        let mut current = start;
        while let Some(referent) = current {
            let Some(instance) = dom.get(referent) else {
                break;
            };
            let class = instance.class();
            let screen = self.database.is_subclass_of(class, "ScreenGui");
            // Both lists for these: the canvas an editor lays one out on is
            // planned with the screens (see `scene::gui::plan`).
            let space = self.database.is_subclass_of(class, "BillboardGui")
                || self.database.is_subclass_of(class, "SurfaceGui");
            if screen || space {
                self.pending.spaces |= space;
                match written && !styling {
                    true => self.pending.screens.root(referent),
                    false => self.pending.screens = Screens::All,
                }
                return;
            }
            current = dom.parent(referent);
        }
        self.pending.screens = Screens::All;
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
        for kind in [
            EffectKind::Particles,
            EffectKind::Beams,
            EffectKind::Trails,
            EffectKind::Highlights,
            EffectKind::Adornments,
        ] {
            let any_now = match kind {
                EffectKind::Particles => false,
                EffectKind::Beams => !self.loaded.scene().beams().is_empty(),
                EffectKind::Trails => !self.loaded.scene().trails().is_empty(),
                // A highlight names its target by referent, never through an
                // `Attachment`, so a moved attachment owes it nothing. An
                // adornment names its adornee the same way.
                EffectKind::Highlights | EffectKind::Adornments => false,
            };
            if !self
                .pending
                .attachment_due(self.pending.effects[Pending::slot(kind)], any_now)
            {
                continue;
            }
            self.loaded.scene_mut().replan_effect(dom, database, kind);
            let world = self.loaded.world();
            let (scene, images) = (world.scene, world.images);
            // Always serves the edit now: a texture this renderer has no
            // upload for draws that effect's own fallback until it lands
            // (see `Renderer::patch_effect`) — never a rebuild.
            self.offscreen.with_renderer(|renderer, device, queue| {
                renderer.patch_effect(device, queue, kind, scene, images)
            });
            self.request_effect_textures(kind);
        }
        let screens = std::mem::take(&mut self.pending.screens);
        match &screens {
            Screens::None => {}
            Screens::All => self.loaded.scene_mut().replan_gui_screens(dom, database),
            Screens::Roots(roots) => {
                // A root the list does not hold yet — one this batch made —
                // has no place in it to go to but the one a whole re-plan
                // finds for it.
                if !self
                    .loaded
                    .scene_mut()
                    .replan_gui_roots(dom, database, roots)
                {
                    self.loaded.scene_mut().replan_gui_screens(dom, database);
                }
            }
        }
        if self.pending.spaces {
            self.loaded.scene_mut().replan_gui_spaces(dom, database);
        }
        if screens.owed() || self.pending.spaces {
            // An `ImageLabel` the edit pointed at an image this session has
            // never seen, and a text object it gave a family this session has
            // never seen, both draw their fallback until the asset lands;
            // asking now is what makes it land.
            self.loaded.resolve_gui_images(self.resident);
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
            let refs: Vec<AssetRef> = match kind {
                EffectKind::Particles => scene
                    .particle_emitters()
                    .iter()
                    .map(|emitter| emitter.texture.clone())
                    .collect(),
                EffectKind::Beams => scene
                    .beams()
                    .iter()
                    .map(|beam| beam.texture.clone())
                    .collect(),
                EffectKind::Trails => scene
                    .trails()
                    .iter()
                    .map(|trail| trail.texture.clone())
                    .collect(),
                // A highlight is drawn from the geometry it covers and two
                // flat colours; there is no image to fetch.
                EffectKind::Highlights => Vec::new(),
                EffectKind::Adornments => scene.adornment_images(),
            };
            let mut wanted = Vec::new();
            for reference in refs {
                if reference != AssetRef::Empty && !wanted.contains(&reference) {
                    wanted.push(reference);
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
