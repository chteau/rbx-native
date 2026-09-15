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
use crate::input::{CameraInput, Input};
use crate::lighting::{self, Lighting};
use crate::load::{Loaded, Toggles};
use crate::quality::QualityLevel;
use crate::scene::Bounds;

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
        let loaded = Loaded::from_dom(&dom, &database, toggles)
            .map_err(|err| format!("nothing to show in {path:?}: {err}"))?;

        let bounds = *loaded.world().scene.bounds();
        let quality = QualityLevel::default();
        Ok(Headless {
            offscreen: Offscreen::new(loaded.world(), &quality.profile())?,
            quality,
            bounds,
            controller: Controller::new(Start::Orbit, &bounds, None, DEFAULT_SENSITIVITY),
            input: Input::default(),
            start: Instant::now(),
            from: Viewpoint::Orbit(0.0),
            toggles,
            database,
            loaded,
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
        let loaded = Loaded::from_dom(dom, &self.database, self.toggles)?;
        let bounds = *loaded.world().scene.bounds();

        let start = match self.from {
            Viewpoint::Orbit(_) => Start::Orbit,
            Viewpoint::Free(pose) => Start::Pose(pose),
        };
        self.offscreen = Offscreen::new(loaded.world(), &self.quality.profile())?;
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

    /// Patches one `BasePart`'s transform/colour/material/transparency/
    /// reflectance in place, for a Properties-panel edit that touches nothing
    /// but that one instance — see `crate::scene::Scene::patch_part` for
    /// exactly what is and is not safe to patch this way.
    ///
    /// `Ok(false)` means the edit crossed a GPU bucket (shape, drawn/
    /// translucent, shadow caster, or landed on a material layer never
    /// uploaded); the caller falls back to [`Headless::reload`].
    pub fn patch_instance(&mut self, dom: &WeakDom, referent: Ref) -> Result<bool, String> {
        let known_material_layers = self.loaded.scene().materials().layers();
        let Some(index) = self.loaded.scene_mut().patch_part(
            dom,
            &self.database,
            referent,
            known_material_layers,
        ) else {
            return Ok(false);
        };
        let part = self.loaded.scene().parts()[index];
        Ok(self.offscreen.patch_instance(&part))
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
        self.offscreen.set_selection(referents);
    }

    /// Advances the camera by `dt` and reports whether the view actually moved,
    /// which is the host's cue to draw a frame: a camera at rest returns `false`
    /// every tick, so an untouched view costs no render and no readback at all.
    pub fn tick(&mut self, dt: Duration) -> bool {
        let from = self
            .controller
            .update(&mut self.input, dt, self.start.elapsed(), &self.bounds);
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
