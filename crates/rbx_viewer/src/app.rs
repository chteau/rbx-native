//! Windowed path: a winit event loop driving the free (or, with `--orbit`,
//! orbiting-then-free) camera.
//!
//! `--quality auto` is managed here too, from the frame times this loop measures
//! (see [`quality`]).

mod fps;
mod input;
mod pacing;
mod quality;
mod title;

use std::sync::Arc;
use std::time::{Duration, Instant};

use wgpu::CurrentSurfaceTexture;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::camera::Viewpoint;
use crate::controller::{Controller, Start};
use crate::gpu;
use crate::input::{CameraInput, Input};
use crate::quality::{QualityLevel, QualityProfile};
use crate::renderer::{Renderer, World};

use fps::FrameRate;
use input::{camera_key, wheel_notches};
use pacing::SmoothedDt;
use quality::Automatic;
use title::title;

const INITIAL_SIZE: (u32, u32) = (1280, 720);
// How long a speed change stays in the title bar before it reverts to the file name.
const SPEED_TITLE_DURATION: Duration = Duration::from_millis(1500);

/// Opens a windowed viewer: by default flies freely from the scene's center right
/// away; with `orbit`, instead orbits the scene until the first right-click or
/// movement key, then flies freely from there.
pub(crate) fn open_window(
    world: World<'_>,
    quality: QualityLevel,
    title: &str,
    orbit: bool,
    speed: Option<f32>,
    sensitivity: f32,
) -> Result<(), String> {
    let event_loop =
        EventLoop::new().map_err(|err| format!("failed to start the event loop: {err}"))?;
    // Poll rather than Wait: even once free, the controller needs a tick every
    // frame to integrate held-key movement, not just on discrete input events.
    event_loop.set_control_flow(ControlFlow::Poll);

    let now = Instant::now();
    let start = if orbit { Start::Orbit } else { Start::Spawn };
    let controller = Controller::new(start, world.scene.bounds(), speed, sensitivity);
    let initial_speed = controller.speed();
    let mut viewer = Viewer {
        world,
        quality,
        profile: quality.profile(),
        automatic: None,
        title,
        start: now,
        last_frame: now,
        motion_dt: SmoothedDt::new(),
        active: None,
        error: None,
        input: Input::default(),
        controller,
        last_shown_speed: initial_speed,
        speed_title_until: None,
        fps: FrameRate::new(now),
    };
    event_loop
        .run_app(&mut viewer)
        .map_err(|err| format!("the event loop failed: {err}"))?;

    viewer.error.map_or(Ok(()), Err)
}

/// Event loop state holder.
///
/// Stores any error that occurs during callbacks (where early returns are not possible),
/// so `open_window` can report it once the event loop stops.
struct Viewer<'a> {
    world: World<'a>,
    quality: QualityLevel,
    /// What the renderer was last set to; `quality` is what was asked for,
    /// which in `Automatic` is not a level at all.
    profile: QualityProfile,
    /// Present only under `--quality auto`, and only once there is a window to
    /// read a refresh rate off.
    automatic: Option<Automatic>,
    title: &'a str,
    start: Instant,
    last_frame: Instant,
    /// De-noised `dt` fed to the controller; see `pacing` for why the raw wall-clock
    /// sample isn't used directly for motion.
    motion_dt: SmoothedDt,
    active: Option<Active>,
    error: Option<String>,
    input: Input,
    controller: Controller,
    last_shown_speed: f32,
    speed_title_until: Option<Instant>,
    /// Per-second frame rate for the title bar. Runs whether the quality level
    /// is pinned or `--quality auto`-managed — unlike `automatic`, which only
    /// exists in the latter case — the same way `rbxstudio`'s corner label
    /// shows its own reading regardless of quality mode.
    fps: FrameRate,
}

/// GPU and window state once a window exists.
///
/// Recreating the surface and renderer when the window is destroyed and later resumed
/// ensures compliance with mobile-style suspend/resume cycles.
struct Active {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
}

impl Viewer<'_> {
    fn activate(&mut self, event_loop: &ActiveEventLoop) -> Result<Active, String> {
        let attributes = Window::default_attributes()
            .with_title(self.title)
            .with_inner_size(LogicalSize::new(INITIAL_SIZE.0, INITIAL_SIZE.1));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|err| format!("failed to open a window: {err}"))?,
        );

        let instance = gpu::instance();
        // The Arc makes the surface outlive the borrow, which is what `'static` asks for.
        let surface = instance
            .create_surface(window.clone())
            .map_err(|err| format!("failed to create a render surface: {err}"))?;
        let adapter = gpu::adapter(&instance, Some(&surface))?;
        let (device, queue) = gpu::device(&adapter)?;

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| "the GPU adapter cannot present to this window".to_string())?;
        // Shading happens on linear values, so an sRGB target is preferred: the hardware
        // re-encodes on write instead of the image coming out washed out.
        let capabilities = surface.get_capabilities(&adapter);
        if let Some(srgb) = capabilities.formats.iter().find(|f| f.is_srgb()) {
            config.format = *srgb;
        }
        // The GUI overlay attaches the non-sRGB twin of that format so it can
        // composite in encoded space (see `renderer::gui::pipeline::encoded`),
        // and a view of a format the configuration never listed is rejected.
        config.view_formats = vec![config.format.remove_srgb_suffix()];
        surface.configure(&device, &config);

        let renderer = Renderer::new(&device, &queue, config.format, self.world, &self.profile);
        self.automatic = Automatic::new(self.quality, &window);

        Ok(Active {
            window,
            surface,
            config,
            device,
            queue,
            renderer,
        })
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame);
        self.last_frame = now;
        // Counts this same redraw, not a second measurement of it — see `fps`.
        let fps_updated = self.fps.record(now);
        let elapsed = self.start.elapsed();
        // Raw `dt` is wall-clock noise around an actually-steady vsync cadence (see
        // `pacing`); motion integration gets the de-noised estimate instead, while the
        // quality manager below still measures the raw sample, since it does need the
        // frame's real cost.
        let motion_dt = self.motion_dt.sample(dt);

        let bounds = self.world.scene.bounds();
        // The windowed interactive viewer has no orthographic toggle of its
        // own (only the offscreen `--orthographic` screenshot path does —
        // see `capture::write_png`), so the wheel always dollies here.
        let from = self
            .controller
            .update(&mut self.input, motion_dt, elapsed, bounds, false);
        let title_update = self.next_title(now, fps_updated);
        // The whole frame, presentation included: what the window actually
        // delivered, which is the only thing the manager can measure here.
        let level = self.automatic.as_mut().and_then(|auto| auto.record(dt));
        if let Some(level) = level {
            self.profile = QualityLevel::Level(level).profile();
        }

        let Some(active) = &mut self.active else {
            return;
        };
        if let Some(text) = title_update {
            active.window.set_title(&text);
        }
        if level.is_some() {
            // Between two frames, never mid-frame: the switch rebuilds bind groups
            // the frame in flight would still be reading.
            active.renderer.set_quality(&active.device, &self.profile);
        }

        if let Err(err) = active.draw(from) {
            self.error = Some(err);
            event_loop.exit();
        }
    }

    /// Whether the title bar should change this frame.
    ///
    /// A fresh speed reading opens the window that shows it; a frame-rate
    /// refresh (once a second, whether or not the speed is also showing) needs
    /// a redraw too, since `title` composes both into the same line.
    fn next_title(&mut self, now: Instant, fps_updated: bool) -> Option<String> {
        let speed = self.controller.speed();
        let mut changed = fps_updated;
        if (speed - self.last_shown_speed).abs() > f32::EPSILON {
            self.last_shown_speed = speed;
            self.speed_title_until = Some(now + SPEED_TITLE_DURATION);
            changed = true;
        }

        let showing_speed = match self.speed_title_until {
            Some(until) if now < until => true,
            Some(_) => {
                self.speed_title_until = None;
                changed = true;
                false
            }
            None => false,
        };

        if !changed {
            return None;
        }
        Some(title(
            self.title,
            self.fps.latest(),
            showing_speed.then(|| speed.round() as i64),
        ))
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, event: KeyEvent) {
        if event.logical_key == Key::Named(NamedKey::Escape) && event.state == ElementState::Pressed
        {
            event_loop.exit();
            return;
        }

        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        let pressed = event.state == ElementState::Pressed;
        if pressed && !event.repeat && matches!(code, KeyCode::KeyF | KeyCode::Home) {
            self.controller.recentre(self.world.scene.bounds());
        }
        if let Some(key) = camera_key(code) {
            self.input.apply(CameraInput::Key { key, pressed });
        }
    }

    fn handle_mouse_button(&mut self, button: MouseButton, state: ElementState) {
        if button != MouseButton::Right {
            return;
        }

        let pressed = state == ElementState::Pressed;
        self.input.apply(CameraInput::LookButton(pressed));

        // Raw deltas (`DeviceEvent::MouseMotion`) drive the look, not the cursor
        // position, so the cursor itself is only ever hidden and pinned in place.
        let Some(active) = &self.active else {
            return;
        };
        if pressed {
            active.window.set_cursor_visible(false);
            if active
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .is_err()
            {
                // X11 doesn't implement `Locked`; `Confined` still keeps the
                // cursor from escaping the window while the drag is held.
                let _ = active.window.set_cursor_grab(CursorGrabMode::Confined);
            }
        } else {
            let _ = active.window.set_cursor_grab(CursorGrabMode::None);
            active.window.set_cursor_visible(true);
        }
    }

    fn release_input(&mut self) {
        self.input.apply(CameraInput::Release);
        if let Some(active) = &self.active {
            let _ = active.window.set_cursor_grab(CursorGrabMode::None);
            active.window.set_cursor_visible(true);
        }
    }
}

impl Active {
    fn draw(&mut self, from: Viewpoint) -> Result<(), String> {
        let frame = match self.surface.get_current_texture() {
            // Suboptimal still hands over a usable frame, and the resize that caused it
            // reconfigures the surface through its own event.
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            // A resize or a compositor change can invalidate the swapchain; reconfiguring
            // and skipping this frame is the documented recovery.
            CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return Ok(()),
            CurrentSurfaceTexture::Validation => {
                return Err("the GPU rejected the surface frame".to_string())
            }
        };

        self.renderer.draw(
            &self.device,
            &self.queue,
            &frame.texture,
            (self.config.width, self.config.height),
            from,
        );
        // Presentation moved onto the queue in wgpu 30; the frame is consumed here.
        self.queue.present(frame);

        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }
}

impl ApplicationHandler for Viewer<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Resuming with a live window (mobile-style suspend cycles) must not rebuild it.
        if self.active.is_some() {
            return;
        }

        match self.activate(event_loop) {
            Ok(active) => {
                self.active = Some(active);
                // Adapter/device setup and the initial texture upload can take
                // anywhere from tens of milliseconds to a few seconds; without this,
                // that whole span reads as the first frame's `dt`, and `motion_dt`'s
                // clamp (see `pacing`) then takes many real frames to decay back down,
                // moving the camera far too fast if a key is already held at launch.
                let now = Instant::now();
                self.last_frame = now;
                // Same reasoning for the title bar's frame rate: `fps` was created
                // before this setup ran, and counting that span as part of its first
                // one-second window would report a fraction of a frame per second.
                self.fps = FrameRate::new(now);
            }
            Err(err) => {
                self.error = Some(err);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event_loop, event),
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_button(button, state)
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let notches = wheel_notches(delta);
                self.input.apply(CameraInput::Wheel { notches });
            }
            WindowEvent::Focused(false) | WindowEvent::CursorLeft { .. } => self.release_input(),
            WindowEvent::Resized(size) => {
                if let Some(active) = &mut self.active {
                    active.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.input.apply(CameraInput::MouseLook {
                dx: delta.0 as f32,
                dy: delta.1 as f32,
            });
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(active) = &self.active {
            active.window.request_redraw();
        }
    }
}
