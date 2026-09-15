//! Embedding API: a place rendered into plain pixel buffers, with no window of
//! its own, for hosts that draw the result themselves (the editor shell does).

use std::path::Path;
use std::time::{Duration, Instant};

use glam::Vec3;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::camera::{self, Pose, Viewpoint};
use crate::capture::{Offscreen, Rendered};
use crate::controller::{Controller, Start, DEFAULT_SENSITIVITY};
use crate::gizmo::Gizmo;
use crate::input::{CameraInput, Input};
use crate::lighting::{self, Lighting};
use crate::load::{Loaded, Toggles};
use crate::quality::QualityLevel;
use crate::scene::{Bounds, EffectKind, MeshPatch};
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
    /// gizmo. Kept here rather than only inside the renderer because a rebuild
    /// throws that one away — see [`View`].
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
    /// an editor calling [`Headless::reload`]/[`Headless::update_lighting`]/
    /// [`Headless::patch_instance`] once per keystroke cannot afford to pay
    /// that again and again.
    database: ReflectionDatabase,
    /// The scene/decor/lighting the current frame draws from, kept (not just
    /// borrowed into `Offscreen::new` and dropped) so
    /// [`Headless::update_lighting`]/[`Headless::patch_instance`] can
    /// recompute their own small piece of it instead of the whole place.
    loaded: Loaded,
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
        let mut loaded = Loaded::from_dom(&dom, &database, toggles)
            .map_err(|err| format!("nothing to show in {path:?}: {err}"))?;
        let warnings = loaded.take_warnings();

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
            warnings,
        })
    }

    /// Rebuilds every render input from a DOM already in memory — a script's
    /// edit, not a file on disk — keeping the current camera pose and quality
    /// level exactly as they were.
    ///
    /// Downloads (textures, materials, meshes) hit `assets`' on-disk cache for
    /// anything the place already showed, so a small edit reloads fast even
    /// though this rebuilds the whole scene rather than patching it in place.
    pub fn reload(&mut self, dom: &WeakDom) -> Result<(), String> {
        let mut loaded = Loaded::from_dom(dom, &self.database, self.toggles)?;
        self.warnings.extend(loaded.take_warnings());
        let bounds = *loaded.world().scene.bounds();

        let start = match self.from {
            Viewpoint::Orbit(_) => Start::Orbit,
            Viewpoint::Free(pose) => Start::Pose(pose),
        };
        // `self.view`, not a fresh one: the renderer being replaced here is
        // the only thing that held the selection outline, the gizmo and the
        // projection mode, and none of them are rebuilt from the DOM.
        self.offscreen = Offscreen::new(loaded.world(), &self.quality.profile(), &self.view)?;
        self.controller = Controller::new(
            start,
            &bounds,
            Some(self.controller.speed()),
            DEFAULT_SENSITIVITY,
        );
        self.bounds = bounds;
        self.loaded = loaded;
        Ok(())
    }

    /// Recomputes only `Lighting`/local lights from `dom` and pushes them to
    /// the GPU in place — no scene rebuild, no texture re-upload. For a
    /// `Lighting`/`Atmosphere`/`Clouds`/`PostEffect`/`Light` edit, which is the
    /// only kind of edit this is ever worth calling for.
    ///
    /// `Ok(false)` when the local light count changed underneath (this never
    /// happens from a plain property edit, only were a script to add or
    /// remove a `Light` through this path instead of a full reload) — the
    /// caller should fall back to [`Headless::reload`] in that case.
    pub fn update_lighting(&mut self, dom: &WeakDom) -> Result<bool, String> {
        let lighting = Lighting::from_dom(dom, &self.database, self.toggles.clock_time);
        // `local_lights` itself has no on/off switch — matching what
        // `load::local_lights` does for a full build keeps a `--no-lights` run's
        // fast path from lighting anything the full path would not have either.
        let lights = if self.toggles.lights {
            lighting::local_lights(dom, &self.database, self.bounds.center())
        } else {
            Vec::new()
        };

        let applied = self.offscreen.update_lighting(lighting, &lights);
        if applied {
            self.loaded.set_lighting(lighting, lights);
        }
        Ok(applied)
    }

    /// Patches one `BasePart` in place for a Properties-panel edit that
    /// touches nothing but that one instance — its transform, colour,
    /// material, transparency, reflectance, shape or shadow flag — see
    /// `crate::scene::Scene::patch_part` for what is and is not safe to
    /// patch this way, and `Scene::patch_mesh_instance` for the same on a
    /// part whose box a resolved `MeshPart`/`SpecialMesh`/union mesh has
    /// replaced. An edit that moves the instance between GPU batches (a
    /// `Transparency` crossing 0, a `CastShadow` toggle, a new `Shape`) is
    /// still a single-instance operation: see `Renderer::sync_instance`.
    ///
    /// `Ok(false)` means only a full reload draws the right picture — a
    /// material layer, mesh or texture never uploaded, a union repainted
    /// from its operation tree, a referent the scene never built — and the
    /// caller falls back to [`Headless::reload`].
    pub fn patch_instance(&mut self, dom: &WeakDom, referent: Ref) -> Result<bool, String> {
        let known_material_layers = self.loaded.scene().materials().layers();
        if let Some(index) =
            self.loaded
                .scene_mut()
                .patch_part(dom, &self.database, referent, known_material_layers)
        {
            let part = self.loaded.scene().parts()[index];
            self.offscreen.sync_instance(dom, &self.database, &part);
            return Ok(true);
        }
        match self.loaded.scene_mut().patch_mesh_instance(
            dom,
            &self.database,
            referent,
            known_material_layers,
        ) {
            Some(MeshPatch::Placed(index)) => {
                let resolved = self.loaded.scene().resolved_file_meshes();
                Ok(self
                    .offscreen
                    .sync_mesh_instance(resolved, &resolved.instances[index]))
            }
            Some(MeshPatch::Removed) => {
                self.offscreen.remove_mesh_instance(referent);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Reflects a `Parent` change that kept `referent` (and everything under
    /// it) inside `Workspace`: nothing visible moves — a part's `CFrame` is
    /// world-space, and every decal, light and effect under it stays under
    /// it — so there is nothing to redraw, and `Ok(true)` says so. See
    /// `crate::scene::Scene::already_draws` for exactly what is checked.
    ///
    /// `Ok(false)` when the subtree crossed the `Workspace` boundary in
    /// either direction (or was never built at all): what to draw changed,
    /// and only a full reload works that out — the caller falls back to
    /// [`Headless::reload`].
    pub fn reparent(&mut self, dom: &WeakDom, referent: Ref) -> Result<bool, String> {
        Ok(self
            .loaded
            .scene()
            .already_draws(dom, &self.database, referent))
    }

    /// Re-plans the `ParticleEmitter`/`Beam`/`Trail` list `referent`'s class
    /// belongs to from `dom` and hands it to the renderer in place — no scene
    /// rebuild, no texture re-upload, no restarted simulation. See
    /// `crate::scene::Scene::replan_effect` for why the whole list and
    /// `crate::renderer::Renderer::patch_effect` for what survives.
    ///
    /// `Ok(false)` when `referent` is none of those three classes, or its
    /// edit named a texture never downloaded; the caller falls back to
    /// [`Headless::reload`].
    pub fn patch_effect(&mut self, dom: &WeakDom, referent: Ref) -> Result<bool, String> {
        let Some(kind) = dom
            .get(referent)
            .and_then(|instance| EffectKind::of(&self.database, instance.class()))
        else {
            return Ok(false);
        };
        self.loaded
            .scene_mut()
            .replan_effect(dom, &self.database, kind);
        Ok(self.offscreen.patch_effect(kind, self.loaded.scene()))
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
    /// ([`Headless::load`]/[`Headless::reload`]): a single-instance patch
    /// never downloads a mesh the scene did not already have.
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
