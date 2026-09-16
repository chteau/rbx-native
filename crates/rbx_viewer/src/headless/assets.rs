//! The streaming half of [`Headless`]: asking for an asset an edit or a load
//! named, drawing the fallback meanwhile, and folding the result into the
//! picture when it lands.
//!
//! # Why the swap-in is a rebuild and not a per-instance sync
//!
//! An asset does not reach one instance. A landed mesh suppresses a part's
//! box, which takes that part's `Decal`s out of the plan; a landed material
//! pack changes every layer that shares it and resizes the texture arrays; a
//! landed sky panel changes the environment probe every pass binds. Doing each
//! of those by hand would be five partial answers where
//! [`crate::renderer::Renderer::rebuild`] is one whole one — and the rebuild is
//! not the expensive thing here anyway: it keeps the device, its pipelines and
//! every upload whose invalidation key says the new scene would upload it
//! identically, so what it actually uploads for one landed asset is that one
//! asset. On the 16k-instance fixture it is about thirty milliseconds, paid
//! once per batch of landings rather than once per landing, and never on the
//! path of the edit that asked.

use std::time::{Duration, Instant};

use rbx_assets::AssetRef;
use rbx_dom::{Ref, WeakDom};

use super::Headless;

/// How long a swap-in waits to collect more landings before rebuilding.
///
/// A cold load's several hundred assets arrive over a second or two on six
/// workers; folding each in the tick it arrives would mean a rebuild every few
/// milliseconds, all of them thrown away by the next. Ten a second is enough
/// that a place fills in visibly while it loads, and few enough that the
/// rebuilds are a fraction of one core. A landing that empties the queue is
/// never held back by it: the last one is what completes the picture.
const SWAP_INTERVAL: Duration = Duration::from_millis(100);

/// The mesh and image references one instance would need, as
/// `Scene::replan_assets_of` reports them.
pub(super) type Wanted = (Vec<AssetRef>, Vec<AssetRef>);

impl Headless {
    /// Collects everything the background pool has finished, and folds it into
    /// the picture once a swap-in is due. `true` when the frame changed.
    pub(super) fn take_landed_assets(&mut self) -> bool {
        let settled = self.resident.poll();
        self.warnings.extend(settled.warnings);
        self.landed.extend(settled.references);
        if self.landed.is_empty() {
            return false;
        }
        // Everything is in: this is the landing that finishes the place, and
        // holding it back would leave the last assets out of the picture for
        // another tenth of a second for no reason.
        let last = self.resident.in_flight() == 0;
        if !last && self.swapped.elapsed() < SWAP_INTERVAL {
            return false;
        }

        let landed = std::mem::take(&mut self.landed);
        self.swapped = Instant::now();
        // A fetch cannot be cancelled, so a `MeshId` typed, corrected and
        // typed again leaves a result arriving for a reference nothing names
        // any more. It stays filed in the `Resident` — the next place to name
        // it gets it free — and costs nothing else.
        if !self.loaded.wants_any(&landed) {
            return false;
        }

        let warnings = self.loaded.resolve(&mut self.resident);
        self.warnings.extend(warnings);
        self.offscreen.reload(self.loaded.world(), &self.view);
        self.uploaded_material_layers = self.loaded.scene().materials().layers();
        self.meshes_changed = true;
        true
    }

    /// Asks for whatever of `wanted` is not resident, and records that the
    /// place is now waiting on it.
    pub(super) fn request(&mut self, wanted: Wanted) {
        let (meshes, images) = wanted;
        if !meshes.is_empty() {
            self.loaded.also_wants(&meshes);
            self.resident.meshes(&meshes);
        }
        if !images.is_empty() {
            self.loaded.also_wants(&images);
            self.resident.images(&images);
        }
    }

    /// Asks for the textures the freshly re-planned effect lists name.
    ///
    /// The whole list rather than the one edited instance: `replan_effect`
    /// rebuilds the list the edited class belongs to, and asking for a
    /// reference already resident costs a lookup.
    pub(super) fn request_effect_images(&mut self) {
        let scene = self.loaded.scene();
        let mut images: Vec<AssetRef> = Vec::new();
        let mut push = |reference: &AssetRef| {
            if *reference != AssetRef::Empty && !images.contains(reference) {
                images.push(reference.clone());
            }
        };
        for emitter in scene.particle_emitters() {
            push(&emitter.texture);
        }
        for beam in scene.beams() {
            push(&beam.texture);
        }
        for trail in scene.trails() {
            push(&trail.texture);
        }
        self.request((Vec::new(), images));
    }

    /// The `Ok(true)` an edit naming geometry nobody has fetched gets instead
    /// of a full reload: the instance goes back to its fallback box now, and
    /// the assets it wants are asked for — see `Scene::fall_back_to_box` for
    /// which referents have a box to go back to and which do not.
    pub(super) fn patch_onto_fallback(
        &mut self,
        dom: &WeakDom,
        referent: Ref,
        wanted: Wanted,
    ) -> Result<bool, String> {
        let known_material_layers = self.uploaded_material_layers;
        let Some(index) = self.loaded.scene_mut().fall_back_to_box(
            dom,
            &self.database,
            referent,
            known_material_layers,
        ) else {
            return Ok(false);
        };

        // The mesh first: its batch and the box's are different passes, and
        // leaving both would draw the instance twice for a frame.
        self.offscreen.remove_mesh_instance(referent);
        let part = self.loaded.scene().parts()[index];
        self.loaded
            .replan_faces(dom, &self.database, referent, &part.placement());
        self.offscreen.sync_instance(dom, &self.database, &part);
        self.request(wanted);
        Ok(true)
    }

    /// Drops what one asset reference decoded to, so the next edit naming it
    /// fetches and decodes it again.
    ///
    /// For the benchmark harness alone: it is how "an edit naming an asset
    /// this session has never seen" is staged against a fixture whose assets
    /// are all in the on-disk cache, without evicting that cache and measuring
    /// a download instead of the path under test. Nothing in the editor calls
    /// this, and nothing should.
    #[doc(hidden)]
    pub fn forget_asset(&mut self, reference: &str) {
        let Ok(reference) = AssetRef::parse(reference) else {
            return;
        };
        self.resident.forget(&reference);
    }

    /// How many asset fetches are still running — the harness's cue that a
    /// streamed load has finished arriving, and nothing else's.
    #[doc(hidden)]
    pub fn assets_in_flight(&self) -> usize {
        self.resident.in_flight()
    }

    /// [`Headless::tick`]'s asset half on its own, without advancing the
    /// camera: `true` when a landing was folded into the picture.
    ///
    /// For the benchmark, which times how long a requested asset takes to
    /// reach a frame and cannot have the orbit camera moving underneath that
    /// measurement. Everything else should tick.
    #[doc(hidden)]
    pub fn swap_assets(&mut self) -> bool {
        self.take_landed_assets()
    }
}
