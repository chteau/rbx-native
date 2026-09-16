//! Embedding API: a place rendered into plain pixel buffers, with no window of
//! its own, for hosts that draw the result themselves (the editor shell does).

mod changes;

use std::path::Path;
use std::time::{Duration, Instant};

use glam::Vec3;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::camera::{self, Pose, Viewpoint};
use crate::capture::{Offscreen, Rendered};
use crate::changes::Roles;
use crate::controller::{Controller, Start, DEFAULT_SENSITIVITY};
use crate::gizmo::Gizmo;
use crate::input::{CameraInput, Input};
use crate::load::{Loaded, Resident, Toggles};
use crate::quality::QualityLevel;
use crate::scene::Bounds;
use crate::view::View;

/// A loaded place that renders frames on demand, flown with the very same
/// free-flight camera as the windowed viewer.
///
/// Holds its own GPU device, so it never shares state with the host's renderer.
/// Every frame is read back over PCIe (about 5 MB at 1500x900), which is cheap
/// enough for a continuously redrawn view but is not free: render at the size
/// actually displayed, and only when [`Headless::tick`] says the view moved.
pub struct Headless {
    offscreen: Offscreen,
    quality: QualityLevel,
    /// What the embedder asked the renderer to show that the place itself does
    /// not say: the projection mode, the outlined selection and the transform
    /// gizmo. Kept here, not only inside the renderer, as the one copy every
    /// rebuild is put back into — see [`View`].
    view: View,
    bounds: Bounds,
    controller: Controller,
    input: Input,
    /// Wall clock the orbit is driven by, so it turns at the same rate however
    /// many frames the host managed to draw.
    start: Instant,
    from: Viewpoint,
    /// What [`Headless::load`] downloaded with: replayed by [`Headless::reload`]
    /// so a command-bar edit keeps the same textures/materials/lights choice.
    toggles: Toggles,
    /// Built once here rather than by every reload: it parses the several-
    /// megabyte embedded API dump (see [`ReflectionDatabase::embedded`]), and
    /// an editor calling [`Headless::apply_changes`] once per keystroke
    /// cannot afford to pay that again and again.
    database: ReflectionDatabase,
    /// The scene/decor/lighting the current frame draws from, kept (not just
    /// borrowed into `Offscreen::new` and dropped) so
    /// [`Headless::apply_changes`] can recompute the one piece of it an edit
    /// touched instead of the whole place.
    loaded: Loaded,
    /// What every instance of the DOM the scene was built from is to the
    /// picture, so a `Change::Removed` — a bare referent, its instance
    /// already gone — can still be taken out of the right pass; see
    /// [`Roles`].
    roles: Roles,
    /// Every asset the place's loads decoded so far, so a reload decodes
    /// only what the place never showed before — see [`Resident`].
    resident: Resident,
    /// Asset-fetch/decode warnings from every [`Headless::load`]/
    /// [`Headless::reload`] so far, not yet claimed by
    /// [`Headless::drain_warnings`] — an embedder (`rbxstudio`'s render
    /// thread) polls this instead of scraping stderr to surface them in its
    /// own UI.
    warnings: Vec<String>,
}

impl Headless {
    /// Parses `path` and uploads it to a GPU of its own.
    ///
    /// Blocks while the place's decals, textures and meshes download, so a host
    /// with a UI thread must call this before it has a window to keep alive.
    /// `textures` off skips those downloads entirely, which is what makes a
    /// network-less or key-less run fast rather than slow.
    ///
    /// The view opens orbiting the place's bounds and flies freely from there on
    /// the first input; [`Headless::open_at`] starts from a pose instead.
    pub fn load(path: &Path, textures: bool) -> Result<Self, String> {
        let toggles = Toggles {
            textures,
            materials: textures,
            lights: true,
            clock_time: None,
        };
        let database = ReflectionDatabase::embedded();
        let dom = crate::load::read_place(path)?;
        let mut resident = Resident::default();
        let mut loaded = Loaded::from_dom(&dom, &database, toggles, &mut resident)
            .map_err(|err| format!("nothing to show in {path:?}: {err}"))?;
        let warnings = loaded.take_warnings();
        let roles = Roles::of_dom(&dom, &database);

        let bounds = *loaded.world().scene.bounds();
        let quality = QualityLevel::default();
        Ok(Headless {
            offscreen: Offscreen::new(loaded.world(), &quality.profile(), &View::default())?,
            quality,
            view: View::default(),
            bounds,
            controller: Controller::new(Start::Orbit, &bounds, None, DEFAULT_SENSITIVITY),
            input: Input::default(),
            start: Instant::now(),
            from: Viewpoint::Orbit(0.0),
            toggles,
            database,
            loaded,
            roles,
            resident,
            warnings,
        })
    }

    /// Rebuilds every render input from a DOM already in memory — a file on
    /// disk plays no part — keeping the current camera pose and quality
    /// level exactly as they were. What [`Headless::apply_changes`] falls
    /// back to for the few edits it names in [`crate::Rebuild`], and what a
    /// host with no change log to offer calls.
    ///
    /// The whole scene is re-derived from the DOM rather than patched, which
    /// is the one answer that is right whatever the edit did; neither the
    /// assets nor the GPU are started over for it. Every asset the place
    /// already decoded is answered from memory (see [`Resident`]), and the
    /// device, its pipelines and every upload the new scene names the same
    /// asset for — material packs, sky, decal images, file meshes, effect
    /// textures — stay where they are (see `renderer::rebuild`); only an
    /// asset the place never showed before is fetched, decoded and uploaded.
    pub fn reload(&mut self, dom: &WeakDom) -> Result<(), String> {
        let mut loaded = Loaded::from_dom(dom, &self.database, self.toggles, &mut self.resident)?;
        self.warnings.extend(loaded.take_warnings());
        let bounds = *loaded.world().scene.bounds();

        let start = match self.from {
            Viewpoint::Orbit(_) => Start::Orbit,
            Viewpoint::Free(pose) => Start::Pose(pose),
        };
        // `self.view`, not a fresh one: the selection outline, the gizmo and
        // the projection mode are not rebuilt from the DOM, and this is the
        // copy of them the rebuild is put back into.
        self.offscreen.reload(loaded.world(), &self.view);
        self.controller = Controller::new(
            start,
            &bounds,
            Some(self.controller.speed()),
            DEFAULT_SENSITIVITY,
        );
        self.bounds = bounds;
        self.loaded = loaded;
        self.roles = Roles::of_dom(dom, &self.database);
        Ok(())
    }

    /// Opens the view standing at `eye` and looking toward `look_at`, both in
    /// studs, at `fov_degrees`, and flies on from there — what a host opening a
    /// place at its own `Camera` uses. Nothing orbits afterwards.
    pub fn open_at(&mut self, eye: [f32; 3], look_at: [f32; 3], fov_degrees: f32) {
        let mut pose = camera::look_at_pose(Vec3::from(eye), Vec3::from(look_at));
        pose.fov_degrees = fov_degrees;
        self.controller =
            Controller::new(Start::Pose(pose), &self.bounds, None, DEFAULT_SENSITIVITY);
        self.from = Viewpoint::Free(pose);
    }

    /// Redraws every following frame at `level`, leaving the camera and the place
    /// itself alone.
    ///
    /// Cheap enough to do between two frames: the images, meshes and instance
    /// buffers a place uploaded are level-independent, so only the bind groups
    /// that view them, the shadow map and — where the sample count changes — the
    /// surface pipelines are rebuilt. Setting the level it is already at costs
    /// nothing.
    pub fn set_quality(&mut self, level: QualityLevel) {
        if level == self.quality {
            return;
        }
        self.quality = level;
        self.offscreen.set_quality(&level.profile());
    }

    /// The level [`Headless::set_quality`] last set.
    pub fn quality(&self) -> QualityLevel {
        self.quality
    }

    /// Swaps the viewport's main camera between perspective and orthographic
    /// (parallel) projection, leaving its position/yaw/pitch and the
    /// WASD/mouse-look controller untouched — see `Camera::with_orthographic`.
    /// Resyncs the free camera's orthographic zoom to a reasonable starting
    /// guess the moment orthographic switches on (see
    /// `Controller::sync_ortho_scale`); the mouse wheel takes over from there.
    pub fn set_orthographic(&mut self, orthographic: bool) {
        if orthographic == self.view.orthographic {
            return;
        }
        self.view.set_orthographic(orthographic);
        if orthographic {
            self.controller.sync_ortho_scale(&self.bounds);
        }
        self.offscreen.set_orthographic(orthographic);
    }

    /// The projection mode [`Headless::set_orthographic`] last set.
    pub fn orthographic(&self) -> bool {
        self.view.orthographic
    }

    pub fn input(&mut self, event: CameraInput) {
        self.input.apply(event);
    }

    /// Outlines `referents`' parts in the viewport — a slice so a future
    /// multi-select has somewhere to grow into — and drops the outline
    /// entirely for a referent with no `BasePart` placement (a `Folder`, a
    /// service, or a `Model`, whose aggregate bounds nothing here computes
    /// yet).
    ///
    /// A host with an idle-skipping render loop (see `Headless::tick`) still
    /// has to force one frame after this: the camera has not moved, but the
    /// picture has.
    pub fn set_selection(&mut self, referents: &[Ref]) {
        self.view.select(referents);
        self.offscreen.set_selection(referents);
    }

    /// Draws the transform tool's axis draggers over the selected part, or
    /// hides them again with `None` — Studio's Select tool, where there is
    /// nothing to drag.
    ///
    /// Where the handles land is worked out per frame from the selection and
    /// the live camera (see `renderer::Renderer::handles`), so this only has
    /// to be called when the tool or its world/local setting changes, not as
    /// the camera moves. The embedder hit-tests the cursor against the same
    /// geometry through [`crate::gizmo`].
    pub fn set_gizmo(&mut self, gizmo: Option<Gizmo>) {
        self.view.set_gizmo(gizmo);
        self.offscreen.set_gizmo(gizmo);
    }

    /// Advances the camera by `dt` and reports whether the view actually moved,
    /// which is the host's cue to draw a frame: a camera at rest returns `false`
    /// every tick, so an untouched view costs no render and no readback at all.
    pub fn tick(&mut self, dt: Duration) -> bool {
        let from = self.controller.update(
            &mut self.input,
            dt,
            self.start.elapsed(),
            &self.bounds,
            self.view.orthographic,
        );
        let moved = from != self.from;
        self.from = from;
        moved
    }

    /// The flight speed in studs per second, for a host that shows it the way
    /// the windowed viewer shows it in its title bar.
    pub fn speed(&self) -> f32 {
        self.controller.speed()
    }

    /// The current free-flight pose, for a host that mirrors it somewhere the
    /// renderer itself never writes to (`rbxstudio`'s DOM sync). `None` while
    /// the view is still auto-orbiting with no pose of its own — see
    /// [`Headless::load`]'s doc comment for when that ends.
    pub fn pose(&self) -> Option<Pose> {
        match self.from {
            Viewpoint::Free(pose) => Some(pose),
            Viewpoint::Orbit(_) => None,
        }
    }

    /// Returns and clears every asset-fetch/decode warning collected by
    /// [`Headless::load`]/[`Headless::reload`] since the last call — an
    /// embedder with nowhere better to look than the return value polls this
    /// instead of scraping stderr. Empty most of the time (nothing failed, or
    /// nothing failed since the last poll), which costs nothing beyond an
    /// empty `Vec`'s allocation-free drop.
    pub fn drain_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Every file mesh the current scene resolved, for a hit test against
    /// the triangles actually drawn — see [`crate::pick::parts_along`]. A
    /// handle onto the renderer's own copies rather than a duplicate of them,
    /// so an embedder can pass it to another thread for the cost of a few
    /// reference counts. Changes only when the scene is rebuilt
    /// ([`Headless::load`]/[`Headless::reload`]): a patch never downloads a
    /// mesh the scene did not already have.
    pub fn pick_meshes(&self) -> crate::pick::Meshes {
        crate::pick::Meshes::new(self.loaded.scene().resolved_file_meshes().meshes.clone())
    }

    /// Draws the current view and returns the frame the *previous* call asked
    /// for: the GPU is left drawing this one while the host uploads that one.
    ///
    /// `None` when there is no earlier frame to hand back — the first call, and
    /// every change of size. A host that stops calling this collects the frame
    /// still in flight with [`Headless::take_frame`], or it never reaches the
    /// screen.
    pub fn render_frame(&mut self, width: u32, height: u32) -> Result<Option<Rendered>, String> {
        self.offscreen.queue_frame((width, height), self.from)
    }

    /// The frame already drawn but not yet handed back, without drawing another:
    /// what a host calls once the view comes to rest.
    pub fn take_frame(&mut self) -> Result<Option<Rendered>, String> {
        self.offscreen.take_frame()
    }
}
