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
use crate::controller::{CameraFeel, Controller, Start, DEFAULT_SENSITIVITY};
use crate::gizmo::Gizmo;
use crate::input::{CameraInput, Input};
use crate::load::{Loaded, Resident, Toggles};
use crate::pick::Selected;
use crate::quality::QualityLevel;
use crate::renderer::{GuiBox, Segment};
use crate::scene::{Bounds, ScrollTarget};
use crate::view::View;

mod assets;

/// One [`Headless::render_gui`] frame.
pub struct GuiCanvas {
    /// Tightly packed RGBA8 (sRGB) rows, `size.0 * size.1 * 4` bytes long.
    pub pixels: Vec<u8>,
    /// What was drawn: the size asked for, or a `BillboardGui`/`SurfaceGui`'s
    /// own canvas size, which is not the caller's to choose.
    pub size: (u32, u32),
    /// The screen's own frame — what its top-level children resolve
    /// against, below any top-bar inset — then every element as laid out,
    /// in paint order: what the canvas is hit-tested against.
    pub boxes: Vec<GuiBox>,
}

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
    /// Put back on every controller a reload or `open_at` builds afresh.
    feel: CameraFeel,
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
    /// Every asset the place's loads decoded so far, and the background pool
    /// that decodes the ones it has not seen — see [`Resident`]. Nothing on
    /// this struct ever waits on it: an asset that is not here yet is drawn as
    /// its fallback and swapped in when it lands (see [`Headless::tick`]).
    resident: Resident,
    /// References answered since the last swap-in, held until one is due.
    landed: Vec<rbx_assets::AssetRef>,
    /// When the last swap-in ran, so a cold load's few hundred assets cost a
    /// bounded number of rebuilds rather than one per tick they trickle in on.
    swapped: Instant,
    /// Whether a swap-in has changed the resolved meshes since the host last
    /// took them — see [`Headless::pick_meshes_changed`].
    meshes_changed: bool,
    /// How many material layers the renderer's texture arrays actually hold.
    ///
    /// Not `scene.materials().layers()`: a single-part edit naming a material
    /// the place had not used adds a layer to the catalog there and then,
    /// while the arrays on the GPU are only ever resized by a rebuild. An
    /// instance written with a layer past this would sample past the end of
    /// them.
    uploaded_material_layers: usize,
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
    /// Returns with the place drawable and its assets still arriving: every
    /// decal, material pack and file mesh is asked for in the background and
    /// swapped in by [`Headless::tick`] as it lands, so the first frame is a
    /// scene of plain boxes under a default sky rather than a wait. That is
    /// what Roblox's own engine does, and it is the only shape that also
    /// answers the harder case — a keystroke naming an asset nobody has
    /// fetched, where there is no "before the window exists" left to hide a
    /// download in. `textures` off skips those requests entirely.
    ///
    /// The view opens orbiting the place's bounds and flies freely from there on
    /// the first input; [`Headless::open_at`] starts from a pose instead.
    pub fn load(path: &Path, textures: bool) -> Result<Self, String> {
        let toggles = Toggles {
            textures,
            materials: textures,
            lights: true,
            clock_time: None,
            show_development_gui: false,
        };
        let database = ReflectionDatabase::embedded();
        let dom = crate::load::read_place(path)?;
        let mut resident = Resident::streaming();
        let mut loaded = Loaded::from_dom(&dom, &database, toggles, &mut resident)
            .map_err(|err| format!("nothing to show in {path:?}: {err}"))?;
        let warnings = loaded.take_warnings();
        let roles = Roles::of_dom(&dom, &database);

        let bounds = *loaded.world().scene.bounds();
        let uploaded_material_layers = loaded.scene().materials().layers();
        let quality = QualityLevel::default();
        Ok(Headless {
            offscreen: Offscreen::new(loaded.world(), &quality.profile(), &View::default())?,
            quality,
            view: View::default(),
            bounds,
            controller: Controller::new(Start::Orbit, &bounds, None, DEFAULT_SENSITIVITY),
            feel: CameraFeel::default(),
            input: Input::default(),
            start: Instant::now(),
            from: Viewpoint::Orbit(0.0),
            toggles,
            database,
            loaded,
            roles,
            resident,
            landed: Vec::new(),
            swapped: Instant::now(),
            meshes_changed: false,
            uploaded_material_layers,
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
    /// textures — stay where they are (see `renderer::rebuild`). An asset the
    /// place never showed before is asked for rather than waited for, exactly
    /// as at load: a reload never blocks on the network either.
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
        self.controller.set_feel(self.feel);
        self.bounds = bounds;
        self.uploaded_material_layers = loaded.scene().materials().layers();
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
        self.controller.set_feel(self.feel);
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

    /// Outlines what `selected` covers in the viewport — a slice so a
    /// multi-select has somewhere to grow into. Each entry names an instance
    /// and the parts it stands for, resolved by the host against its own DOM
    /// (`crate::pick::Selected`): a `Model` or a `Folder` has no placement to
    /// outline itself, so what is drawn for one is the box around the parts
    /// beneath it, and only a container with none of those draws nothing.
    ///
    /// A host with an idle-skipping render loop (see `Headless::tick`) still
    /// has to force one frame after this: the camera has not moved, but the
    /// picture has.
    pub fn set_selection(&mut self, selected: &[Selected]) {
        self.view.select(selected);
        self.offscreen.set_selection(selected, self.loaded.scene());
    }

    /// Whether scene geometry standing in front of the selection hides its
    /// outline. `false` by default: the box is a control the user is acting
    /// on rather than scenery, so it shows through whatever it is behind —
    /// see `renderer::selection`. Same idle-skipping caveat as
    /// [`Headless::set_selection`]: nothing moved, but the picture changed.
    pub fn set_selection_occluded(&mut self, occluded: bool) {
        if occluded == self.view.selection_occluded {
            return;
        }
        self.view.set_selection_occluded(occluded);
        self.offscreen
            .set_selection_occluded(occluded, self.loaded.scene());
    }

    /// Whether [`Headless::set_selection_occluded`] last asked for an
    /// occluded selection box.
    pub fn selection_occluded(&self) -> bool {
        self.view.selection_occluded
    }

    /// Outlines whatever `BasePart` the cursor is over, distinctly from the
    /// selection outline above — Studio's "about to click" cue. `None` clears
    /// it. Unlike `set_selection`, the caller is expected to have already
    /// resolved this down to a `BasePart` referent (or nothing) rather than
    /// walking up to an enclosing `Model`: there is no click here to apply
    /// Studio's "select the model" convention to, only a cursor position.
    ///
    /// Same idle-skipping caveat as `set_selection`: force a frame afterwards
    /// if the host's render loop only draws on camera movement.
    pub fn set_hover(&mut self, selected: Vec<Selected>) {
        self.view.set_hover(selected.clone());
        self.offscreen.set_hover(selected, self.loaded.scene());
    }

    /// Draws ghost boxes where a tool being configured would put things —
    /// the Align tool's live preview (`studio/align-tool.md`: "dynamically
    /// previewing the point of alignment before confirming"). An empty
    /// list clears them, which is what closing the tool sends.
    ///
    /// Plain world matrices rather than anything read out of the DOM: the
    /// whole point is a placement no instance has yet.
    pub fn set_preview(&mut self, boxes: Vec<glam::Mat4>) {
        self.offscreen.set_preview(&boxes);
        self.view.set_preview(boxes);
    }

    /// Draws world-space line segments over the scene — Studio's light
    /// guides (see [`crate::light_guides`]), an editor's dragger guides.
    /// Replaces every segment sent before on `layer` alone: each source of
    /// lines has a layer of its own, so one that changes every mouse move
    /// never re-uploads another that has not. An empty list clears the
    /// layer. Kept across a reload like the selection is.
    pub fn set_lines(&mut self, layer: usize, segments: Vec<Segment>) {
        self.offscreen.set_lines(layer, &segments);
        self.view.set_lines(layer, segments);
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
    /// Lays the `ScreenGui` overlay out as a `screen`-pixel device's screen
    /// and draws it scaled onto each frame, whatever size that is: an editor
    /// emulating the device a GUI was designed for, in a view of its own
    /// size. `None` goes back to laying it out at the frame's size.
    /// [`Headless::gui_scroll_target`] keeps taking points in the frame's
    /// pixels either way.
    pub fn set_gui_screen(&mut self, screen: Option<(u32, u32)>) {
        self.offscreen
            .with_renderer(|renderer, _, _| renderer.set_gui_screen(screen));
    }

    pub fn set_gizmo(&mut self, gizmo: Option<Gizmo>) {
        self.view.set_gizmo(gizmo);
        self.offscreen.set_gizmo(gizmo);
    }

    /// The `ScreenGui` `ScrollingFrame` the mouse wheel over `point` — in
    /// pixels of the last frame drawn — would scroll along `axis` (0 across,
    /// 1 down), with how far its `CanvasPosition` can go. The innermost
    /// frame under the point whose canvas overflows its window on that axis;
    /// `None` over anything else, which is what leaves the wheel to the
    /// camera. Only the screen overlay is tested: a `SurfaceGui`/
    /// `BillboardGui` canvas has no cursor in pixels to test against.
    ///
    /// Scrolling it is the host's job: it writes `CanvasPosition` and hands
    /// the change back through [`Headless::apply_changes`], and the next
    /// frame lays the overlay out again.
    pub fn gui_scroll_target(&self, point: [f32; 2], axis: usize) -> Option<ScrollTarget> {
        self.offscreen.renderer().gui_scroll_target(point, axis)
    }

    /// Draws one `ScreenGui` on its own — enabled or not — laid out against a
    /// `width`×`height` screen (a `BillboardGui`/`SurfaceGui` at its own
    /// canvas size instead), over a flat `backdrop` (encoded sRGB) with no
    /// scene behind it: an editor's 2D canvas, through the very layout and
    /// painter the overlay uses. Synchronous, unlike
    /// [`Headless::render_frame`]: call it when the tree changed, not every
    /// frame.
    pub fn render_gui(
        &mut self,
        screen: Ref,
        (width, height): (u32, u32),
        backdrop: [f32; 3],
    ) -> Result<GuiCanvas, String> {
        self.offscreen.gui_canvas((width, height), screen, backdrop)
    }

    /// Advances the camera by `dt` and reports whether the next frame would
    /// differ from the last: the view moved, or an asset landed and was swapped
    /// into the picture. A camera at rest over a place whose assets are all in
    /// returns `false` every tick, so an untouched view costs no render and no
    /// readback at all.
    ///
    /// This is also the one place a background fetch is ever collected — see
    /// `Headless::take_landed_assets`. A host that stops ticking stops
    /// streaming, which is the right way round: a viewport nobody is looking
    /// at has nothing to swap anything into.
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
        // Not `||`: the assets have to be collected whether the camera moved
        // or not, and short-circuiting would leave them waiting for a tick
        // that happens to be still.
        moved | self.take_landed_assets()
    }

    /// How the free camera responds to the mouse and the keys, from now on.
    pub fn set_camera_feel(&mut self, feel: CameraFeel) {
        self.feel = feel;
        self.controller.set_feel(feel);
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
    /// reference counts. Changes when the scene is rebuilt
    /// ([`Headless::load`]/[`Headless::reload`]) and when an asset lands and
    /// is swapped in — see [`Headless::pick_meshes_changed`], which is how a
    /// host notices the second of those without a command having been applied.
    pub fn pick_meshes(&self) -> crate::pick::Meshes {
        crate::pick::Meshes::new(self.loaded.scene().resolved_file_meshes().meshes.clone())
    }

    /// Whether a landed asset has changed the geometry a click is tested
    /// against since the last call, and clears that. A host that holds only a
    /// handle onto [`Headless::pick_meshes`] has to take the new one, or a
    /// click on a mesh that has just streamed in would be tested against a set
    /// that did not have it.
    pub fn pick_meshes_changed(&mut self) -> bool {
        std::mem::take(&mut self.meshes_changed)
    }

    /// How many recovered fallback pieces the picture draws `referent` as —
    /// zero for everything but a legacy `UnionOperation`/`NegateOperation`
    /// whose boolean could not be computed, which is drawn as the additive
    /// parts recovered from its operation tree instead (see `scene::union`).
    ///
    /// Public because a union drawn that way is the one instance a place
    /// draws as several, and nothing about its class says so: which of the
    /// two a given union is depends on what its asset carved to. A harness
    /// or a test that means to exercise that case has no other way to find
    /// one, and guessing from the class would measure whichever kind the
    /// fixture happened to list first.
    pub fn fallback_pieces(&self, referent: Ref) -> usize {
        self.loaded.scene().piece_count(referent) as usize
    }

    /// Draws the current view and returns the frame the *previous* call asked
    /// for: the GPU is left drawing this one while the host uploads that one.
    ///
    /// `None` when there is no earlier frame to hand back — the first call, and
    /// every change of size. A host that stops calling this collects the frame
    /// still in flight with [`Headless::take_frame`], or it never reaches the
    /// screen.
    pub fn render_frame(&mut self, width: u32, height: u32) -> Result<Option<Rendered>, String> {
        self.offscreen.set_elapsed(self.start.elapsed());
        self.offscreen.queue_frame((width, height), self.from)
    }

    /// The frame already drawn but not yet handed back, without drawing another:
    /// what a host calls once the view comes to rest.
    pub fn take_frame(&mut self) -> Result<Option<Rendered>, String> {
        self.offscreen.take_frame()
    }
}
