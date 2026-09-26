//! The browser build: [`Viewer`], which a page drives from JavaScript (see
//! `web/index.html`). It is the windowed viewer's loop (`app`) with the
//! browser standing in for winit — the page forwards its keyboard, pointer
//! and wheel events as [`CameraInput`], and calls [`Viewer::frame`] from
//! `requestAnimationFrame` — and the editor's streaming loader (`Headless`)
//! with the browser's `fetch()` standing in for the worker pool: the place
//! draws at once, on fallbacks, and every mesh, texture, union and font is
//! swapped in as it arrives from `rbxview --serve` (see `crate::serve`).
//!
//! WebGPU only: wgpu's WebGL2 path lacks storage buffers and compute, which
//! the renderer leans on.

use std::path::Path;
use std::time::Duration;

use glam::Vec2;
use rbx_assets::AssetRef;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use wasm_bindgen::prelude::*;
use web_time::Instant;

use crate::camera::Viewpoint;
use crate::controller::{Controller, Start, DEFAULT_SENSITIVITY};
use crate::fps::{readout, FrameRate};
use crate::input::{CameraInput, CameraKey, Input};
use crate::load::{self, Loaded, Resident, Toggles};
use crate::pacing::SmoothedDt;
use crate::pick;
use crate::quality::{Automatic, QualityLevel, QualityProfile};
use crate::renderer::Renderer;
use crate::services;

/// How long a swap-in waits to collect more landings — `headless::assets`'
/// `SWAP_INTERVAL`, for the same reason.
const SWAP_INTERVAL: Duration = Duration::from_millis(100);

#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

/// What the page has switched off. Profile knobs, applied over whatever the
/// quality level (or `Automatic`) picked.
#[derive(Clone, Copy)]
struct Shown {
    gui: bool,
    particles: bool,
    beams: bool,
    trails: bool,
    shadows: bool,
    bloom: bool,
    color_correction: bool,
}

#[wasm_bindgen]
pub struct Viewer {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,

    /// The place as parsed, never edited: a scene toggle rebuilds from it
    /// (from a copy, for `ShowDevelopmentGui`), so every referent the page
    /// holds — the selection, the Explorer's rows — stays good.
    dom: WeakDom,
    name: String,
    /// What is selected, in the order it was, as Studio keeps it.
    selected: Vec<Ref>,
    /// Where the last frame was drawn from, which a click is a ray through.
    from: Viewpoint,
    database: ReflectionDatabase,
    toggles: Toggles,
    resident: Resident,
    loaded: Loaded,
    landed: Vec<AssetRef>,
    swapped: Instant,
    warnings: Vec<String>,

    controller: Controller,
    input: Input,
    quality: QualityLevel,
    automatic: Option<Automatic>,
    shown: Shown,
    start: Instant,
    last_frame: Instant,
    motion_dt: SmoothedDt,
    fps: FrameRate,
}

#[wasm_bindgen]
impl Viewer {
    /// Opens `bytes` (a `.rbxl`/`.rbxm`/`.rbxlx`/`.rbxmx`, named `name` in
    /// errors) on `canvas`, fetching assets from `assets` — the URL of a
    /// `rbxview --serve`'s `/asset` route.
    pub async fn open(
        canvas: web_sys::HtmlCanvasElement,
        bytes: Vec<u8>,
        name: String,
        assets: String,
    ) -> Result<Viewer, JsError> {
        let size = (canvas.width().max(1), canvas.height().max(1));
        let instance = crate::gpu::instance();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|err| JsError::new(&format!("no WebGPU canvas: {err}")))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|err| JsError::new(&format!("no WebGPU adapter: {err}")))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rbxview"),
                ..Default::default()
            })
            .await
            .map_err(|err| JsError::new(&format!("no WebGPU device: {err}")))?;

        let mut config = surface
            .get_default_config(&adapter, size.0, size.1)
            .ok_or_else(|| JsError::new("the adapter cannot present to this canvas"))?;
        // A canvas is only ever `bgra8unorm`/`rgba8unorm`. The renderer
        // shades in linear light and wants the hardware to encode on write,
        // so it draws through the `-srgb` view of it, the way the native
        // window picks an sRGB surface outright; the GUI pass's own view is
        // the canvas's plain format (see `renderer::gui::pipeline::encoded`).
        config.format = config.format.remove_srgb_suffix();
        let format = config.format.add_srgb_suffix();
        config.view_formats = vec![format];
        surface.configure(&device, &config);

        let toggles = Toggles {
            textures: true,
            materials: true,
            lights: true,
            clock_time: None,
            show_development_gui: false,
        };
        let database = ReflectionDatabase::embedded();
        let mut resident = Resident::served_from(&assets);
        let dom = parse(&bytes, &name)?;
        let mut loaded = build(&dom, &name, &database, toggles, &mut resident)?;
        let warnings = loaded.take_warnings();

        let quality = QualityLevel::Automatic;
        let shown = Shown {
            gui: true,
            particles: true,
            beams: true,
            trails: true,
            shadows: true,
            bloom: true,
            color_correction: true,
        };
        let mut profile = quality.profile();
        apply(&mut profile, shown);
        let renderer = Renderer::new(&device, &queue, format, loaded.world(), &profile);
        let controller = Controller::new(
            Start::Orbit,
            loaded.scene().bounds(),
            None,
            DEFAULT_SENSITIVITY,
        );
        let now = Instant::now();

        Ok(Viewer {
            surface,
            config,
            device,
            queue,
            renderer,
            dom,
            name,
            selected: Vec::new(),
            from: Viewpoint::Orbit(0.0),
            database,
            toggles,
            resident,
            loaded,
            landed: Vec::new(),
            swapped: now,
            warnings,
            controller,
            input: Input::default(),
            quality,
            // A browser does not say what the display refreshes at; the
            // manager's own 60 Hz fallback is what `requestAnimationFrame`
            // runs at on most of them.
            automatic: Automatic::new(quality, None),
            shown,
            start: now,
            last_frame: now,
            motion_dt: SmoothedDt::new(),
            fps: FrameRate::new(now),
        })
    }

    /// Replaces the place, keeping the device, the options and every asset
    /// already decoded. The view starts orbiting the new place.
    pub fn load(&mut self, bytes: Vec<u8>, name: String) -> Result<(), JsError> {
        self.dom = parse(&bytes, &name)?;
        self.name = name;
        self.selected.clear();
        self.rebuild()?;
        self.controller = Controller::new(
            Start::Orbit,
            self.loaded.scene().bounds(),
            Some(self.controller.speed()),
            DEFAULT_SENSITIVITY,
        );
        Ok(())
    }

    /// Switches one option: `textures`, `materials`, `lights`,
    /// `developmentGui`, `gui`, `particles`, `beams`, `trails`, `shadows`,
    /// `bloom` or `colorCorrection`.
    pub fn set(&mut self, option: &str, on: bool) -> Result<(), JsError> {
        let toggles = &mut self.toggles;
        let shown = &mut self.shown;
        match option {
            "textures" => toggles.textures = on,
            "materials" => toggles.materials = on,
            "lights" => toggles.lights = on,
            "developmentGui" => toggles.show_development_gui = on,
            "gui" => shown.gui = on,
            "particles" => shown.particles = on,
            "beams" => shown.beams = on,
            "trails" => shown.trails = on,
            "shadows" => shown.shadows = on,
            "bloom" => shown.bloom = on,
            "colorCorrection" => shown.color_correction = on,
            _ => return Err(JsError::new(&format!("no option {option:?}"))),
        }
        match option {
            "textures" | "materials" | "lights" | "developmentGui" => self.rebuild(),
            // Profile switches, but these four passes only read theirs when
            // they are built — one switched off builds nothing to switch back
            // on — so the scene is rebuilt around the new profile (keeping
            // every upload, as an asset swap-in does).
            "gui" | "particles" | "beams" | "trails" => {
                self.retune();
                self.renderer
                    .rebuild(&self.device, &self.queue, self.loaded.world());
                self.outline();
                Ok(())
            }
            _ => {
                self.retune();
                Ok(())
            }
        }
    }

    /// `0` for Roblox's `Automatic`, else a level from 1 to 21.
    pub fn set_quality(&mut self, level: u8) {
        self.quality = match level {
            0 => QualityLevel::Automatic,
            level => QualityLevel::Level(level),
        };
        self.automatic = Automatic::new(self.quality, None);
        self.retune();
    }

    /// Resizes the drawing buffer; the page passes the canvas's CSS size
    /// times `devicePixelRatio`.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Stands at `eye` looking toward `at` (studs) and flies on from there —
    /// what the page's `?eye=x,y,z&at=x,y,z` asks for, the same view
    /// `rbxview --eye --look-at` frames a screenshot at.
    pub fn look_at(&mut self, eye: Vec<f32>, at: Vec<f32>) -> Result<(), JsError> {
        let (&[ex, ey, ez], &[ax, ay, az]) = (&eye[..], &at[..]) else {
            return Err(JsError::new("eye and at are three numbers each"));
        };
        let pose =
            crate::camera::look_at_pose(glam::Vec3::new(ex, ey, ez), glam::Vec3::new(ax, ay, az));
        self.controller = Controller::new(
            Start::Pose(pose),
            self.loaded.scene().bounds(),
            Some(self.controller.speed()),
            DEFAULT_SENSITIVITY,
        );
        Ok(())
    }

    /// A key going down or up, by `KeyboardEvent.code`. `true` when the
    /// camera took it, so the page can `preventDefault` exactly those.
    pub fn key(&mut self, code: &str, pressed: bool) -> bool {
        if pressed && matches!(code, "KeyF" | "Home") {
            self.controller.recentre(self.loaded.scene().bounds());
            return true;
        }
        let Some(key) = camera_key(code) else {
            return false;
        };
        self.input.apply(CameraInput::Key { key, pressed });
        true
    }

    /// The look button (the right mouse button) going down or up.
    pub fn look(&mut self, pressed: bool) {
        self.input.apply(CameraInput::LookButton(pressed));
    }

    /// Relative pointer motion in CSS pixels (`movementX`/`movementY`).
    pub fn mouse_move(&mut self, dx: f32, dy: f32) {
        self.input.apply(CameraInput::MouseLook { dx, dy });
    }

    /// Wheel travel in notches, positive away from the user.
    pub fn wheel(&mut self, notches: f32) {
        self.input.apply(CameraInput::Wheel { notches });
    }

    /// Lets go of every held key and button — the page lost focus.
    pub fn release(&mut self) {
        self.input.apply(CameraInput::Release);
    }

    /// Advances the camera, folds in whatever assets landed and draws.
    pub fn frame(&mut self) -> Result<(), JsError> {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame);
        self.last_frame = now;
        self.fps.record(now);
        let motion_dt = self.motion_dt.sample(dt);
        let from = self.controller.update(
            &mut self.input,
            motion_dt,
            self.start.elapsed(),
            self.loaded.scene().bounds(),
            false,
        );
        self.from = from;
        if self
            .automatic
            .as_mut()
            .and_then(|auto| auto.record(dt))
            .is_some()
        {
            self.retune();
        }
        self.swap_assets();

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(())
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(JsError::new("the GPU rejected the canvas frame"))
            }
        };
        self.renderer.set_elapsed(self.start.elapsed());
        self.renderer.draw(
            &self.device,
            &self.queue,
            &frame.texture,
            (self.config.width, self.config.height),
            from,
        );
        self.queue.present(frame);
        Ok(())
    }

    /// A click on the canvas at `x`, `y` (drawing-buffer pixels), selecting
    /// the way Studio does (see `pick::from_click`): plain, the outermost
    /// `Model` around the nearest part; `cycle` (Alt), the next part behind
    /// the one selected; `extend` (Shift or Ctrl), added to the selection, or
    /// taken out of it if it was in. A click on nothing clears a plain
    /// selection. Returns the selection.
    pub fn click(&mut self, x: f32, y: f32, extend: bool, cycle: bool) -> Vec<u32> {
        let size = Vec2::new(self.config.width as f32, self.config.height as f32);
        let projection = self.renderer.view_projection(self.from, size.x / size.y);
        let ray = pick::ray_through(projection, pick::ndc_of(Vec2::new(x, y), size));
        let meshes = pick::Meshes::new(self.loaded.scene().resolved_file_meshes().meshes.clone());
        let hits = pick::parts_along(&self.dom, &self.database, &meshes, ray);
        let current = self.selected.last().copied();
        let picked = pick::from_click(&self.dom, &self.database, &hits, current, cycle);
        match (picked, extend) {
            (Some(picked), true) => match self.selected.iter().position(|&s| s == picked) {
                Some(index) => {
                    self.selected.remove(index);
                }
                None => self.selected.push(picked),
            },
            (Some(picked), false) => self.selected = vec![picked],
            (None, true) => {}
            (None, false) => self.selected.clear(),
        }
        self.outline();
        self.selection()
    }

    /// Replaces the selection — an Explorer click.
    pub fn select(&mut self, ids: Vec<u32>) {
        self.selected = ids
            .into_iter()
            .map(Ref::new)
            .filter(|&referent| self.dom.get(referent).is_some())
            .collect();
        self.outline();
    }

    pub fn selection(&self) -> Vec<u32> {
        self.selected.iter().map(Ref::value).collect()
    }

    /// The Explorer rows under `parent` as JSON (`[{id, name, class,
    /// children}]`), or the place's roots for `None` — ordered and filtered
    /// the way Studio's default Explorer view shows them.
    pub fn children(&self, parent: Option<u32>) -> String {
        let nodes: Vec<Node> = match parent {
            Some(parent) => self
                .dom
                .get(Ref::new(parent))
                .map(|instance| instance.children().to_vec())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|child| self.node(child))
                .collect(),
            None => {
                let mut roots: Vec<Node> = self
                    .dom
                    .root_refs()
                    .iter()
                    .filter_map(|&root| self.node(root))
                    .filter(|node| services::is_default_visible(&node.class))
                    .collect();
                roots.sort_by_cached_key(|node| {
                    (
                        services::rank(&node.class).unwrap_or(usize::MAX),
                        node.name.to_lowercase(),
                    )
                });
                roots
            }
        };
        let rows: Vec<_> = nodes
            .iter()
            .map(|node| {
                serde_json::json!({
                    "id": node.id,
                    "name": node.name,
                    "class": node.class,
                    "children": node.children,
                })
            })
            .collect();
        serde_json::Value::from(rows).to_string()
    }

    /// `id` and every instance above it, root first — the rows the Explorer
    /// opens to show a selection made in the viewport.
    pub fn ancestors(&self, id: u32) -> Vec<u32> {
        let mut chain = vec![id];
        let mut current = Ref::new(id);
        while let Some(parent) = self.dom.parent(current) {
            chain.push(parent.value());
            current = parent;
        }
        chain.reverse();
        chain
    }

    /// An instance's name, for the page's selection path.
    pub fn describe(&self, id: u32) -> String {
        self.dom
            .get(Ref::new(id))
            .map_or_else(String::new, |instance| instance.name().to_string())
    }

    pub fn class_of(&self, id: u32) -> String {
        self.dom
            .get(Ref::new(id))
            .map_or_else(String::new, |instance| instance.class().to_string())
    }

    /// One status line: frame rate, flight speed, the quality level in
    /// force and how many assets are still on their way.
    pub fn status(&self) -> String {
        let mut parts = Vec::new();
        if let Some(fps) = self.fps.latest() {
            parts.push(readout(fps));
        }
        parts.push(format!("{:.0} studs/s", self.controller.speed()));
        let level = self.profile_level();
        parts.push(match self.quality {
            QualityLevel::Automatic => format!("quality auto ({level})"),
            QualityLevel::Level(_) => format!("quality {level}"),
        });
        let loading = self.resident.in_flight();
        if loading > 0 {
            parts.push(format!("{loading} assets loading"));
        }
        parts.join(" \u{b7} ")
    }

    /// Every asset warning since the last call.
    pub fn warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

impl Viewer {
    /// Re-reads the place under the current toggles, keeping the renderer's
    /// uploads and every decoded asset — `Headless::reload`, from bytes.
    fn rebuild(&mut self) -> Result<(), JsError> {
        let mut loaded = build(
            &self.dom,
            &self.name,
            &self.database,
            self.toggles,
            &mut self.resident,
        )?;
        self.warnings.extend(loaded.take_warnings());
        self.renderer
            .rebuild(&self.device, &self.queue, loaded.world());
        self.loaded = loaded;
        self.outline();
        Ok(())
    }

    /// Pushes the quality level and the page's switches to the renderer.
    fn retune(&mut self) {
        let mut profile = QualityLevel::Level(self.profile_level()).profile();
        apply(&mut profile, self.shown);
        self.renderer.set_quality(&self.device, &profile);
    }

    /// The level drawn at right now: the pinned one, or the one `Automatic`
    /// has settled on.
    fn profile_level(&self) -> u8 {
        match (self.quality, &self.automatic) {
            (QualityLevel::Automatic, Some(auto)) => auto.level(),
            (quality, _) => quality.resolved(),
        }
    }

    /// `Headless::take_landed_assets`, onto the canvas's renderer.
    fn swap_assets(&mut self) {
        let settled = self.resident.poll();
        self.warnings.extend(settled.warnings);
        self.landed.extend(settled.references);
        if self.landed.is_empty() {
            return;
        }
        let last = self.resident.in_flight() == 0;
        if !last && self.swapped.elapsed() < SWAP_INTERVAL {
            return;
        }
        let landed = std::mem::take(&mut self.landed);
        if !self.loaded.wants_any(&landed) {
            return;
        }
        self.swapped = Instant::now();
        let warnings = self.loaded.resolve(&mut self.resident);
        self.warnings.extend(warnings);
        self.renderer
            .rebuild(&self.device, &self.queue, self.loaded.world());
        self.outline();
    }

    /// Draws the selection's outline — what `Headless::set_selection` does
    /// for the editor.
    fn outline(&mut self) {
        let selected = pick::selection(&self.dom, &self.database, &self.selected);
        self.renderer
            .set_selection(&self.device, &self.queue, &selected, self.loaded.scene());
    }

    fn node(&self, referent: Ref) -> Option<Node> {
        let instance = self.dom.get(referent)?;
        Some(Node {
            id: referent.value(),
            name: instance.name().to_string(),
            class: instance.class().to_string(),
            children: instance.children().len(),
        })
    }
}

/// One Explorer row, as [`Viewer::children`] hands it to the page.
struct Node {
    id: u32,
    name: String,
    class: String,
    children: usize,
}

fn parse(bytes: &[u8], name: &str) -> Result<WeakDom, JsError> {
    load::parse_place(bytes, Path::new(name)).map_err(|err| JsError::new(&err))
}

fn build(
    dom: &WeakDom,
    name: &str,
    database: &ReflectionDatabase,
    toggles: Toggles,
    resident: &mut Resident,
) -> Result<Loaded, JsError> {
    let forced;
    let dom = match toggles.show_development_gui {
        true => {
            let mut copy = dom.clone();
            load::show_development_gui(&mut copy);
            forced = copy;
            &forced
        }
        false => dom,
    };
    Loaded::from_dom(dom, database, toggles, resident)
        .map_err(|err| JsError::new(&format!("nothing to show in {name}: {err}")))
}

fn apply(profile: &mut QualityProfile, shown: Shown) {
    profile.gui = shown.gui;
    profile.particles = shown.particles;
    profile.beams = shown.beams;
    profile.trails = shown.trails;
    profile.shadows &= shown.shadows;
    profile.bloom &= shown.bloom;
    profile.color_correction &= shown.color_correction;
}

/// `app::input::camera_key`, by `KeyboardEvent.code` — the same physical
/// positions, so AZERTY flies with ZQSD here too.
fn camera_key(code: &str) -> Option<CameraKey> {
    match code {
        "KeyW" | "ArrowUp" => Some(CameraKey::Forward),
        "KeyS" | "ArrowDown" => Some(CameraKey::Back),
        "KeyA" | "ArrowLeft" => Some(CameraKey::Left),
        "KeyD" | "ArrowRight" => Some(CameraKey::Right),
        "KeyE" => Some(CameraKey::Up),
        "KeyQ" => Some(CameraKey::Down),
        "ShiftLeft" | "ShiftRight" => Some(CameraKey::Slow),
        _ => None,
    }
}
