//! `Headless::apply_changes`: a change log applied to the picture one
//! instance at a time, the way Roblox's own engine applies a property write
//! — no rebuild, no re-walk of the place, a cost that scales with the edit.
//!
//! Each instance the log names is looked up in the DOM as it stands *now*
//! and classified by [`Role`]; that decides which of the renderer's passes
//! has to be told. A part is re-derived and its records rewritten, moved,
//! added or dropped; a decal is re-projected onto its part; a light, an
//! effect or a GUI marks its whole (cheap, CPU-only) list for a re-plan at
//! the end of the batch, so a hundred lights moved in one script cost one
//! buffer write rather than a hundred. Only the few things named in
//! [`Rebuild`] end in a reload.

mod pending;

use rbx_dom::{Change, Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::Headless;
use crate::capture::Offscreen;
use crate::changes::{fold, holds_gui, Applied, Known, Rebuild, Role, Roles, Touched};
use crate::load::{Loaded, Resident, Toggles};
use crate::scene::{descendants_of, Bounds, PartSync};
use crate::textures;
use pending::Pending;

impl Headless {
    /// Brings the picture in line with `dom` for every instance `changes`
    /// names — and only those — patching each in place. A `Change` is read
    /// as a hint about *which* instance may differ, never as a record of a
    /// value: whether the referent is now a part to draw, a part to drop,
    /// or a decal to re-project is decided by looking it up in `dom`, so an
    /// undo can hand over the very log its mutation produced and the
    /// restored DOM is what gets drawn. A `Model` reparented reaches its
    /// whole subtree; a property write reaches the one instance and what
    /// hangs off it (a moved part carries its decals, lights, emitters and
    /// attachments along).
    ///
    /// [`Applied::Rebuilt`] means one of the changes was one of the few
    /// that only a full [`Headless::reload`] can draw — see [`Rebuild`] for
    /// the complete list — and that reload has already been done; nothing
    /// is left for the caller to do either way. A patch that got part-way
    /// before hitting such a change is simply overtaken by the rebuild.
    pub fn apply_changes(&mut self, dom: &WeakDom, changes: &[Change]) -> Result<Applied, String> {
        let touched = fold(changes);
        let known_layers = self.loaded.scene().materials().layers();
        let mut patcher = Patcher {
            dom,
            database: &self.database,
            roles: &mut self.roles,
            loaded: &mut self.loaded,
            resident: &mut self.resident,
            offscreen: &mut self.offscreen,
            toggles: self.toggles,
            known_layers,
            pending: Pending::default(),
        };
        match patcher.run(&touched) {
            Ok(bounds) => {
                if let Some(bounds) = bounds {
                    // The controller reads these every tick; unlike a reload,
                    // nothing about the camera is started over for them.
                    self.bounds = bounds;
                }
                Ok(Applied::Patched)
            }
            Err(why) => {
                self.reload(dom)?;
                Ok(Applied::Rebuilt(why))
            }
        }
    }
}

/// Everything one batch of changes reaches, borrowed apart so the scene can
/// be read while the renderer is written.
struct Patcher<'a> {
    dom: &'a WeakDom,
    database: &'a ReflectionDatabase,
    roles: &'a mut Roles,
    loaded: &'a mut Loaded,
    resident: &'a mut Resident,
    offscreen: &'a mut Offscreen,
    toggles: Toggles,
    /// The material catalog's layer count as the renderer uploaded it, read
    /// once before the first part is touched: a part that lands on a layer
    /// past it needs maps only a reload uploads, and a refused part must
    /// not raise the bar for the next one.
    known_layers: usize,
    pending: Pending,
}

impl Patcher<'_> {
    /// The new extent, if the parts touched moved it.
    fn run(&mut self, touched: &[Touched]) -> Result<Option<Bounds>, Rebuild> {
        for entry in touched {
            self.touch(entry)?;
        }
        self.finish()
    }

    fn touch(&mut self, touched: &Touched) -> Result<(), Rebuild> {
        let dom = self.dom;
        let referent = touched.referent;
        if dom.get(referent).is_none() {
            return self.forget(referent);
        }
        if !touched.structural {
            return self.present(referent, true);
        }
        if let Some(old) = touched.old_parent {
            self.left(old, referent)?;
        }
        // Pre-order, so a part is in place before the decals, lights and
        // emitters under it read where it stands. Each descendant answers
        // for itself here, which is why `present` is not asked to look at
        // children again.
        let subtree: Vec<Ref> = descendants_of(dom, referent).collect();
        for instance in subtree {
            self.present(instance, false)?;
        }
        Ok(())
    }

    /// The part a `SpecialMesh` or `SurfaceAppearance` was just taken away
    /// from draws differently without it — as its own box again, or bare —
    /// and nothing in the DOM as it stands still leads from the child to it.
    /// The same goes for the container a GUI element left: a GUI tree is
    /// planned from its `ScreenGui`/`BillboardGui`/`SurfaceGui` down, so the
    /// container the element now sits in is not the only one whose plan is
    /// stale — left alone, the old one keeps drawing the element too, and
    /// a `Frame` dragged from an overlay onto a part shows up in both.
    fn left(&mut self, old_parent: Ref, moved: Ref) -> Result<(), Rebuild> {
        let dom = self.dom;
        let role = dom
            .get(moved)
            .map(|instance| Role::of(self.database, instance.class()));
        match role {
            Some(Role::MeshChild | Role::Appearance) => self.sync_parent_part(old_parent),
            // The face itself is taken off the GPU by `present` below, which
            // finds it under something that is not a part any more; the plan
            // the old part left behind still names it.
            Some(Role::Face) => {
                self.loaded.replan_faces(dom, self.database, old_parent);
                Ok(())
            }
            Some(Role::Gui) => {
                self.gui_changed(Some(old_parent));
                Ok(())
            }
            // A `Folder` (or any other plain container) draws nothing itself,
            // yet a GUI tree may hang off one, and the whole subtree moves
            // with it — so the container it left is stale for the same
            // reason a `Frame`'s would be.
            Some(_) if holds_gui(dom, self.database, moved) => {
                self.gui_changed(Some(old_parent));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// An instance the DOM has: (re)built into whichever pass its role says.
    /// `with_children` re-reads what hangs off a part — a property write on
    /// the part alone moved them; a structural walk visits them itself.
    fn present(&mut self, referent: Ref, with_children: bool) -> Result<(), Rebuild> {
        let dom = self.dom;
        let Some(instance) = dom.get(referent) else {
            return Ok(());
        };
        let role = Role::of(self.database, instance.class());
        self.roles.insert(
            referent,
            Known {
                role,
                parent: dom.parent(referent),
            },
        );
        match role {
            Role::Part => self.sync_part(referent, with_children),
            Role::Face => self.sync_face(referent),
            Role::Light => {
                self.pending.lights = true;
                Ok(())
            }
            Role::Lighting => {
                self.pending.lighting = true;
                Ok(())
            }
            Role::Sky => Err(Rebuild::Sky),
            Role::Effect(kind) => {
                self.pending.effect(kind);
                Ok(())
            }
            Role::Attachment => {
                self.pending.attachment();
                Ok(())
            }
            Role::MeshChild | Role::Appearance => match dom.parent(referent) {
                Some(parent) => self.sync_parent_part(parent),
                None => Ok(()),
            },
            Role::Gui => {
                self.gui_changed(Some(referent));
                Ok(())
            }
            Role::Material => Err(Rebuild::Materials),
            Role::Inert => Ok(()),
        }
    }

    /// An instance the DOM no longer has: taken out of the pass its
    /// remembered role put it in.
    fn forget(&mut self, referent: Ref) -> Result<(), Rebuild> {
        let Some(known) = self.roles.remove(referent) else {
            return Ok(());
        };
        match known.role {
            Role::Part => {
                let dropped = self.loaded.scene_mut().remove_part(referent);
                self.render_part(referent, &PartSync::gone(dropped));
                self.pending.parts = true;
                Ok(())
            }
            Role::Face => {
                self.offscreen
                    .with_renderer(|renderer, _, _| renderer.remove_face(referent));
                if let Some(parent) = known.parent {
                    self.loaded.replan_faces(self.dom, self.database, parent);
                }
                Ok(())
            }
            Role::Light => {
                self.pending.lights = true;
                Ok(())
            }
            Role::Lighting => {
                self.pending.lighting = true;
                Ok(())
            }
            Role::Sky => Err(Rebuild::Sky),
            Role::Effect(kind) => {
                self.pending.effect(kind);
                Ok(())
            }
            Role::Attachment => {
                self.pending.attachment();
                Ok(())
            }
            Role::MeshChild | Role::Appearance => match known.parent {
                Some(parent) => self.sync_parent_part(parent),
                None => Ok(()),
            },
            Role::Gui => {
                self.gui_changed(known.parent);
                Ok(())
            }
            Role::Material => Err(Rebuild::Materials),
            Role::Inert => Ok(()),
        }
    }

    /// Re-derives one part and rewrites its records; with `with_children`,
    /// re-reads everything drawn on, at or from it as well.
    fn sync_part(&mut self, referent: Ref, with_children: bool) -> Result<(), Rebuild> {
        let sync = self.loaded.scene_mut().resync_part(
            self.dom,
            self.database,
            referent,
            self.known_layers,
            &self.resident.unions,
        )?;
        if !self.render_part(referent, &sync) {
            return Err(Rebuild::Asset);
        }
        // Whatever this referent now needs — whether it patched onto a
        // resolved mesh/union or fell back to its box because one is not
        // resident yet — is asked for here: a `resync_part` box-fallback
        // draws right away, but only this keeps it from staying a box
        // forever.
        self.request_assets_of(referent);
        // Keeps the decor plan's idea of where this part's own `Decal`s
        // project in step with where it actually stands now, the way
        // `render_part` just kept the GPU in step — without it, a decal
        // image landing later would re-assemble at the placement this
        // part had when the file was read.
        self.loaded.replan_faces(self.dom, self.database, referent);
        self.pending.parts = true;
        // A canvas adorned to this part hangs off it from anywhere in the
        // tree, so the children below are not the only thing that moved.
        if self.loaded.scene().adorns(referent) {
            self.pending.spaces = true;
        }
        // A 3D adornment is placed against the part it adorns in the same
        // way, and its geometry is worked out once at plan time rather than
        // instanced per part — so a part that moves under one re-plans them.
        if self.loaded.scene().adornments_cover(referent) {
            self.pending.effect(crate::scene::EffectKind::Adornments);
        }
        if with_children {
            self.sync_children(referent)?;
        }
        Ok(())
    }

    /// `sync_part` for the parent a mesh child or appearance hangs off, if
    /// that parent is a part at all: the shape or the skin it draws with is
    /// read off its children.
    fn sync_parent_part(&mut self, parent: Ref) -> Result<(), Rebuild> {
        let dom = self.dom;
        let is_part = dom
            .get(parent)
            .is_some_and(|instance| Role::of(self.database, instance.class()) == Role::Part);
        if is_part {
            self.sync_part(parent, true)?;
        }
        Ok(())
    }

    fn sync_children(&mut self, referent: Ref) -> Result<(), Rebuild> {
        let dom = self.dom;
        let Some(instance) = dom.get(referent) else {
            return Ok(());
        };
        for &child in instance.children() {
            let Some(instance) = dom.get(child) else {
                continue;
            };
            match Role::of(self.database, instance.class()) {
                Role::Face => self.sync_face(child)?,
                Role::Light => self.pending.lights = true,
                Role::Effect(kind) => self.pending.effect(kind),
                Role::Attachment => self.pending.attachment(),
                Role::Gui => self.pending.spaces = true,
                _ => {}
            }
        }
        Ok(())
    }

    /// Asks the background loader for whatever `referent` now needs that
    /// this session does not have — see `Scene::wanted_assets_of`. Cheap to
    /// call whether or not anything is actually missing: `Resident`/
    /// `Loaded` skip a reference already resident or already asked for.
    fn request_assets_of(&mut self, referent: Ref) {
        let wanted = self
            .loaded
            .scene_mut()
            .wanted_assets_of(self.dom, self.database, referent);
        if !wanted.meshes.is_empty() {
            self.loaded.also_wants(&wanted.meshes);
            self.resident.meshes(&wanted.meshes);
        }
        if !wanted.unions.is_empty() {
            self.loaded.also_wants(&wanted.unions);
            self.resident.bytes(&wanted.unions);
        }
        if !wanted.images.is_empty() {
            self.loaded.also_wants(&wanted.images);
            self.resident.images(&wanted.images);
        }
    }

    fn render_part(&mut self, referent: Ref, sync: &PartSync) -> bool {
        let resolved = self.loaded.scene().resolved_file_meshes();
        self.offscreen.with_renderer(|renderer, device, queue| {
            renderer.sync_part(device, queue, resolved, referent, sync)
        })
    }

    /// Re-projects one `Decal`/`Texture` onto its part as the scene now
    /// draws it, or takes the projection out: the part is gone, draws
    /// through a mesh, is not one the decal could be pinned to, or textures
    /// are off for this load. An image that was asked for and failed, or
    /// has not been asked for at all yet, is left unpainted, exactly as a
    /// full build leaves an image it does not have — never a rebuild — and
    /// an image nobody has asked for is asked for now, the same as
    /// `sync_part`'s own fallback does for a mesh.
    fn sync_face(&mut self, referent: Ref) -> Result<(), Rebuild> {
        let dom = self.dom;
        let painted = self
            .toggles
            .textures
            .then(|| dom.parent(referent))
            .flatten();
        // Before the projection below and whatever it finds: the plan is
        // what a later asset landing re-assembles every decal from, so it
        // has to be re-read from the DOM even when this face itself ends up
        // unpainted — see `Loaded::replan_faces`.
        if let Some(parent) = painted {
            self.loaded.replan_faces(dom, self.database, parent);
        }
        let face = painted.and_then(|parent| {
            let placement = self.loaded.scene().placement_of(parent)?;
            textures::faces(dom, self.database, parent, &placement)
                .into_iter()
                .find(|(_, face)| face.referent == referent)
        });
        let Some((reference, face)) = face else {
            self.offscreen
                .with_renderer(|renderer, _, _| renderer.remove_face(referent));
            return Ok(());
        };
        let synced = self
            .offscreen
            .with_renderer(|renderer, device, _| renderer.sync_face(device, &reference, &face));
        if !synced && !self.resident.image_failed(&reference) {
            self.loaded.also_wants(std::slice::from_ref(&reference));
            self.resident.images(std::slice::from_ref(&reference));
        }
        Ok(())
    }
}
