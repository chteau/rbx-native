//! The streaming half of [`Headless`]: folding a landed asset into the
//! picture once one arrives from the background pool an edit or a load asked
//! it to fetch (see `headless::changes::Patcher::request_assets_of` and
//! `Loaded::resolve` for the asking and joining halves).
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
        // A fetch cannot be cancelled, so a `MeshId` typed, corrected and
        // typed again leaves a result arriving for a reference nothing names
        // any more. It stays filed in the `Resident` — the next place to name
        // it gets it free — and costs nothing else. The throttle is not
        // touched on the way out: nothing was swapped in, so the next batch
        // that matters must not be held back on this one's account.
        if !self.loaded.wants_any(&landed) {
            return false;
        }

        self.swapped = Instant::now();
        let warnings = self.loaded.resolve(&mut self.resident);
        self.warnings.extend(warnings);
        self.offscreen.reload(self.loaded.world(), &self.view);
        self.uploaded_material_layers = self.loaded.scene().materials().layers();
        self.meshes_changed = true;
        true
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
