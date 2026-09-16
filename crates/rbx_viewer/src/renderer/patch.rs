//! Bringing a built renderer in line with one changed instance at a time —
//! what `Headless::apply_changes` drives, once per instance a change log
//! names, instead of rebuilding every batch from the scene.

use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::{lighting, shadow, Renderer, World};
use crate::camera::Camera;
use crate::lighting::{Lighting, LocalLight};
use crate::scene::{Bounds, EffectKind, PartSync, Resolved, Scene};
use crate::textures::FaceInstance;

impl Renderer {
    /// Brings every pass in line with what `Scene::resync_part` now holds
    /// for `referent`: a box's opaque or blended instance and its shadow
    /// caster are each rewritten in place, moved to another batch (a new
    /// shape, a `Transparency` that crossed 0, a `CastShadow` toggle), added
    /// or dropped, and the selection outline follows its placement; a mesh
    /// instance the same through the file-mesh batches, with any box record
    /// the referent used to have taken out; and a referent gone from the
    /// scene is taken out of everything. A shape the place never used
    /// before gets its unit mesh built here (see `Meshes::ensure`).
    ///
    /// `false` when a mesh batch the instance now belongs in would need a
    /// mesh or texture `resolved` never downloaded — the scene's own check
    /// already refused that, so this is a defensive answer rather than a
    /// case a caller has to reason about.
    pub(crate) fn sync_part(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resolved: &Resolved,
        referent: Ref,
        sync: &PartSync,
    ) -> bool {
        match sync {
            PartSync::Box(part) => {
                self.meshes.ensure(device, part.kind);
                self.shaped.sync(device, part);
                self.translucent.sync(device, part);
                self.shadows.sync_caster(device, part);
                self.selection.place(part.referent, part.placement());
                self.hover.place(device, part.referent, part.placement());
                self.filemesh.remove(referent);
                self.shadows.remove_mesh_caster(referent);
                true
            }
            PartSync::Mesh(index) => {
                let instance = &resolved.instances[*index];
                self.shaped.remove(referent);
                self.translucent.remove(referent);
                self.shadows.remove_caster(referent);
                self.selection.remove(referent);
                self.hover.remove(device, referent);
                self.filemesh.sync(device, queue, resolved, instance)
                    && self.shadows.sync_mesh_caster(device, resolved, instance)
            }
            PartSync::Gone => {
                self.shaped.remove(referent);
                self.translucent.remove(referent);
                self.shadows.remove_caster(referent);
                self.selection.remove(referent);
                self.hover.remove(device, referent);
                self.filemesh.remove(referent);
                self.shadows.remove_mesh_caster(referent);
                true
            }
        }
    }

    /// Rewrites, moves or adds one `Decal`/`Texture`'s projection — see
    /// `textured::Textured::sync`. `false` when `reference` is an image this
    /// renderer never uploaded, which only a reload fetches.
    pub(crate) fn sync_face(
        &mut self,
        device: &wgpu::Device,
        reference: &AssetRef,
        face: &FaceInstance,
    ) -> bool {
        self.textured.sync(device, reference, face)
    }

    /// Drops one `Decal`/`Texture`'s projection — its part stopped drawing
    /// as a box, or the instance itself is gone. A no-op for a referent no
    /// batch holds.
    pub(crate) fn remove_face(&mut self, referent: Ref) {
        self.textured.remove(referent);
    }

    /// Uploads every instance record the edits since the last frame left
    /// owed to a batch buffer, one write per buffer touched — see
    /// `slots::Slots::flush`. [`Renderer::draw`] runs it before any pass
    /// reads a buffer; nothing else needs to.
    pub(super) fn flush_writes(&mut self, queue: &wgpu::Queue) {
        self.shaped.flush(queue);
        self.shadows.flush(queue);
        self.filemesh.flush(queue);
        self.textured.flush(queue);
    }

    /// Applies a `Lighting`/`Atmosphere`/`Clouds`/`PostEffect` edit. The
    /// constant terms (`sun_direction`, `fog`, `clouds`, `Effects`, …) are
    /// folded into the per-frame uniform anyway (see [`Renderer::draw`]),
    /// so swapping the copy in is the whole of it.
    pub(crate) fn set_lighting(&mut self, lighting: Lighting) {
        self.lighting = lighting;
        self.post.set_effects(lighting.effects);
    }

    /// Replaces the place's local lights with `lights` — rewritten into the
    /// buffer they already have where the level's cap leaves the count the
    /// same, or into a freshly sized one where a light came or went, which
    /// every camera bind group then has to be told about (the buffer is
    /// bound, not copied). A single write is the whole cost of moving the
    /// part a light hangs off; a new buffer is the cost of adding one.
    pub(crate) fn set_lights(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lights: &[LocalLight],
    ) {
        let allowed = lights.len().min(self.quality.local_lights_max);
        if allowed == self.lights {
            lighting::local_lights_write(queue, &self.lights_buffer, &lights[..allowed]);
        } else {
            self.lights_buffer = lighting::local_lights_buffer(device, &lights[..allowed]);
            self.light_shadows_buffer = shadow::local::buffer(device, allowed);
            self.lights = allowed;
            self.rebind_frames(device);
        }
        self.all_lights = lights.to_vec();
    }

    /// Hands the renderer `scene`'s freshly re-planned effects of `kind` (see
    /// `crate::scene::Scene::replan_effect`), keeping every texture already
    /// uploaded and every running simulation or recorder that still applies
    /// — see each pass's own `replace` for exactly what survives.
    ///
    /// `false` when a definition names a texture this renderer never tried
    /// to download, which only a full reload fetches.
    pub(crate) fn patch_effect(&mut self, kind: EffectKind, scene: &Scene) -> bool {
        match kind {
            EffectKind::Particles => self.particles.replace(scene.particle_emitters()),
            EffectKind::Beams => self.beams.replace(scene.beams()),
            EffectKind::Trails => self.trails.replace(scene.trails()),
        }
    }

    /// Rebuilds the GUI passes around `world`'s re-planned trees (see
    /// `Scene::replan_gui`) — the canvases are baked again, the atlas keeps
    /// every image it already holds and takes on any `ImageLabel` image the
    /// edit first named out of `Decor::gui`.
    pub(crate) fn refresh_gui(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        world: World<'_>,
    ) {
        let quality = self.quality;
        self.gui.rebuild(
            device,
            queue,
            (world.scene.gui_screens(), world.scene.gui_spaces()),
            &world.decor.gui,
            &quality,
        );
    }

    /// Re-frames on a scene whose extent moved, exactly as a rebuild does:
    /// the orbit camera and the sun shadow's fit both work against it.
    pub(crate) fn set_bounds(&mut self, bounds: Bounds) {
        self.bounds = bounds;
        self.camera = Camera::framing(&bounds).with_orthographic(self.camera.is_orthographic());
    }
}
