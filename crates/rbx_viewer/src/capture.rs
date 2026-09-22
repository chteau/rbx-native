//! Offscreen path: renders frames into a texture the GPU reads straight back,
//! either written out as a PNG or handed to an embedder (see [`crate::headless`]).

mod readback;

use std::path::Path;
use std::time::{Duration, Instant};

use glam::Vec3;

use crate::camera::{self, Camera, Viewpoint};
use crate::gizmo::Gizmo;
use crate::gpu;
use crate::pick::Selected;
use crate::quality::QualityProfile;
use crate::renderer::{Renderer, Segment, World};
use crate::scene::Scene;
use crate::view::View;
use readback::{Pending, Target, FORMAT};

/// A finished frame and where its time went.
///
/// The two costs are what an embedder needs to see to know which end of the
/// pipeline is the slow one. They do not split GPU from CPU: the draw is only
/// *queued* during `render`, so the time the GPU spends on it lands in
/// `readback`, where it is waited for.
pub struct Rendered {
    /// Tightly packed RGBA8 (sRGB) rows, `width * height * 4` bytes long.
    pub pixels: Vec<u8>,
    pub render: Duration,
    pub readback: Duration,
}

/// A renderer drawing into its own texture rather than into a window surface.
pub(crate) struct Offscreen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    target: Option<Target>,
    /// The frame already queued, waiting to be handed back by the next call.
    pending: Option<Pending>,
    /// What [`Offscreen::gui_canvas`] draws into: a size of its own, so an
    /// editor switching between the scene and a canvas never resizes the
    /// other's target out from under it.
    canvas: Option<Target>,
}

impl Offscreen {
    /// Opens a GPU device of its own, builds a renderer for `world` on it and
    /// puts that straight into `view`.
    ///
    /// `view` is not optional on purpose: the state that does not come from
    /// the place has to be said out loud wherever a renderer is built or
    /// rebuilt (see [`Offscreen::reload`]), which is what stops one from
    /// silently starting blank — see [`View`].
    pub(crate) fn new(
        world: World<'_>,
        quality: &QualityProfile,
        view: &View,
    ) -> Result<Self, String> {
        let instance = gpu::instance();
        let adapter = gpu::adapter(&instance, None)?;
        let (device, queue) = gpu::device(&adapter)?;
        let renderer = Renderer::new(&device, &queue, FORMAT, world, quality);

        let mut offscreen = Offscreen {
            device,
            queue,
            renderer,
            target: None,
            pending: None,
            canvas: None,
        };
        offscreen.set_orthographic(view.orthographic);
        offscreen.set_selection(&view.selected, world.scene);
        offscreen.set_selection_occluded(view.selection_occluded, world.scene);
        offscreen.set_hover(view.hovered.clone(), world.scene);
        offscreen.set_preview(&view.preview);
        for (layer, segments) in view.lines.iter().enumerate() {
            offscreen.set_lines(layer, segments);
        }
        offscreen.set_gizmo(view.gizmo);
        Ok(offscreen)
    }

    /// Rebuilds the renderer around `world` on the device it already has —
    /// see [`Renderer::rebuild`] for what that keeps — and puts it back into
    /// `view`, for the same reason [`Offscreen::new`] takes one. The frame
    /// still in flight, if any, is left to be collected as usual: it is a
    /// finished picture of the previous scene, not a stale one.
    pub(crate) fn reload(&mut self, world: World<'_>, view: &View) {
        self.renderer.rebuild(&self.device, &self.queue, world);
        self.set_orthographic(view.orthographic);
        self.set_selection(&view.selected, world.scene);
        self.set_selection_occluded(view.selection_occluded, world.scene);
        self.set_hover(view.hovered.clone(), world.scene);
        self.set_preview(&view.preview);
        for (layer, segments) in view.lines.iter().enumerate() {
            self.set_lines(layer, segments);
        }
        self.set_gizmo(view.gizmo);
    }

    /// Moves the renderer to another graphics quality level, in place: see
    /// [`Renderer::set_quality`] for why that costs no upload and no rebatching.
    pub(crate) fn set_quality(&mut self, quality: &QualityProfile) {
        self.renderer.set_quality(&self.device, quality);
    }

    /// Looks from another height, in degrees above the target.
    pub(crate) fn pitch(&mut self, degrees: f32) {
        self.renderer.pitch(degrees);
    }

    /// Swaps the main camera between perspective and orthographic projection.
    pub(crate) fn set_orthographic(&mut self, orthographic: bool) {
        self.renderer.set_orthographic(orthographic);
    }

    /// Replaces the outlined selection box(es), rebuilding their tiny vertex
    /// buffer right away.
    pub(crate) fn set_selection(&mut self, selected: &[Selected], scene: &Scene) {
        self.renderer
            .set_selection(&self.device, &self.queue, selected, scene);
    }

    /// Whether geometry in front of the selection hides its outline.
    pub(crate) fn set_selection_occluded(&mut self, occluded: bool, scene: &Scene) {
        self.renderer
            .set_selection_occluded(&self.device, &self.queue, occluded, scene);
    }

    /// Replaces the hover outline box, rebuilding its tiny vertex buffer
    /// right away.
    pub(crate) fn set_hover(&mut self, selected: Vec<Selected>, scene: &Scene) {
        self.renderer
            .set_hover(&self.device, &self.queue, selected, scene);
    }

    /// Replaces the ghost boxes a tool is previewing.
    pub(crate) fn set_preview(&mut self, boxes: &[glam::Mat4]) {
        self.renderer.set_preview(&self.device, boxes);
    }

    /// Replaces the editor's line segments.
    pub(crate) fn set_lines(&mut self, layer: usize, segments: &[Segment]) {
        self.renderer.set_lines(&self.device, layer, segments);
    }

    /// Shows or hides the transform tool's draggers over the selection.
    pub(crate) fn set_gizmo(&mut self, gizmo: Option<Gizmo>) {
        self.renderer.set_gizmo(gizmo);
    }

    /// The renderer, read-only: for what an embedder asks of the last frame
    /// rather than writes into the next (see `Headless::gui_scroll_target`).
    pub(crate) fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// The renderer with the device and queue it draws through, for an
    /// edit applied one instance at a time (see `Headless::apply_changes`):
    /// every per-instance write the renderer offers needs the same two
    /// handles, and one door for all of them beats a forwarder each.
    pub(crate) fn with_renderer<T>(
        &mut self,
        f: impl FnOnce(&mut Renderer, &wgpu::Device, &wgpu::Queue) -> T,
    ) -> T {
        f(&mut self.renderer, &self.device, &self.queue)
    }

    /// Forwards to [`Renderer::finish_loading`] — for [`write_png`], which
    /// draws exactly one frame and so has no later frame to finish the load
    /// spread across (see [`Renderer::draw`]'s own texture-upload budget).
    pub(crate) fn finish_loading(&mut self) {
        self.renderer.finish_loading(&self.device, &self.queue);
    }

    /// Draws one frame and waits for it, for a caller that wants this very view
    /// and nothing after it.
    pub(crate) fn frame(&mut self, size: (u32, u32), from: Viewpoint) -> Result<Vec<u8>, String> {
        match self.queue_frame(size, from)? {
            Some(rendered) => Ok(rendered.pixels),
            None => self
                .take_frame()?
                .map(|rendered| rendered.pixels)
                .ok_or_else(|| "the rendered frame went missing".to_string()),
        }
    }

    /// Queues the next frame and returns the one the previous call queued, so the
    /// GPU draws while the CPU is still busy with the frame before it.
    ///
    /// `None` on the first call, and after every size change: there is no earlier
    /// frame, or none of a size the caller still wants.
    pub(crate) fn queue_frame(
        &mut self,
        size: (u32, u32),
        from: Viewpoint,
    ) -> Result<Option<Rendered>, String> {
        if size.0 == 0 || size.1 == 0 {
            return Err(format!("cannot render a {}x{} frame", size.0, size.1));
        }
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.size() != size)
        {
            self.discard();
            self.target = Some(Target::new(&self.device, size));
        }
        // Just assigned above when it was missing or stale.
        let Some(target) = &mut self.target else {
            return Err("the offscreen target went missing".to_string());
        };

        let queued = Instant::now();
        self.renderer
            .draw(&self.device, &self.queue, target.texture(), size, from);
        let pending = target.copy(&self.device, &self.queue);
        let render = queued.elapsed();

        let Some(previous) = self.pending.replace(pending) else {
            return Ok(None);
        };
        let waited = Instant::now();
        let pixels = target.collect(&self.device, previous)?;

        Ok(Some(Rendered {
            pixels,
            render,
            readback: waited.elapsed(),
        }))
    }

    /// Hands back the queued frame without drawing another, which is how the last
    /// frame before the view came to rest still reaches the screen.
    pub(crate) fn take_frame(&mut self) -> Result<Option<Rendered>, String> {
        let (Some(pending), Some(target)) = (self.pending.take(), &self.target) else {
            return Ok(None);
        };

        let waited = Instant::now();
        let pixels = target.collect(&self.device, pending)?;
        Ok(Some(Rendered {
            pixels,
            render: Duration::ZERO,
            readback: waited.elapsed(),
        }))
    }

    /// Draws `screen` alone over `backdrop` (see `Renderer::draw_gui_canvas`)
    /// and waits for it: a canvas is redrawn only when something on it
    /// changed, so there is no stream of frames to overlap the readback with.
    /// Returns the pixels and every element as laid out.
    pub(crate) fn gui_canvas(
        &mut self,
        size: (u32, u32),
        screen: rbx_dom::Ref,
        backdrop: [f32; 3],
    ) -> Result<(Vec<u8>, Vec<crate::renderer::GuiBox>), String> {
        if size.0 == 0 || size.1 == 0 {
            return Err(format!("cannot render a {}x{} canvas", size.0, size.1));
        }
        let target = match &mut self.canvas {
            Some(target) if target.size() == size => target,
            canvas => canvas.insert(Target::new(&self.device, size)),
        };
        let [r, g, b] = backdrop.map(f64::from);
        self.renderer.draw_gui_canvas(
            &self.device,
            &self.queue,
            target.texture(),
            (size, screen),
            wgpu::Color { r, g, b, a: 1.0 },
        );
        let pending = target.copy(&self.device, &self.queue);
        let pixels = target.collect(&self.device, pending)?;
        Ok((pixels, self.renderer.gui_boxes().to_vec()))
    }

    /// Drops the queued frame, mapping and all: a readback buffer left mapped is
    /// one the next copy into it cannot use.
    fn discard(&mut self) {
        if let (Some(pending), Some(target)) = (self.pending.take(), &self.target) {
            let _ = target.collect(&self.device, pending);
        }
    }
}

/// How `write_png` frames its single offscreen frame — every `--yaw`/
/// `--pitch`/`--eye`/`--look-at`/`--orthographic` option `rbxview`'s CLI
/// takes, bundled together so the function they configure stays under
/// clippy's argument-count lint rather than growing a ninth positional `bool`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Framing {
    pub(crate) yaw: Option<f32>,
    pub(crate) pitch: Option<f32>,
    /// When given, places the camera directly instead of framing the scene's
    /// bounds — the only way to get close enough to examine a small prop in a
    /// huge map — and takes over from `yaw`/`pitch` entirely, matching how
    /// the free camera's pose already overrides the orbit camera in the
    /// windowed path.
    pub(crate) eye_look_at: Option<(Vec3, Vec3)>,
    pub(crate) orthographic: bool,
}

/// Renders a single frame offscreen and writes it as PNG.
pub(crate) fn write_png(
    world: World<'_>,
    quality: &QualityProfile,
    output: &Path,
    size: (u32, u32),
    framing: Framing,
) -> Result<(), String> {
    // A single capture outlines nothing and has no tool active; only the
    // projection mode is ever asked for from the command line.
    let view = View {
        orthographic: framing.orthographic,
        ..View::default()
    };
    let mut offscreen = Offscreen::new(world, quality, &view)?;
    // A single offscreen frame is drawn below and nothing after it, so there
    // is no later frame for `Renderer::draw`'s own texture-upload budget to
    // spread the rest of the load across — finish it now instead of writing
    // out a PNG with some textures still on their placeholder.
    offscreen.finish_loading();

    if let Some(degrees) = framing.pitch {
        offscreen.pitch(degrees);
    }

    let from = match framing.eye_look_at {
        Some((eye, look_at)) => Viewpoint::Free(camera::look_at_pose(eye, look_at)),
        None => Viewpoint::Orbit(Camera::screenshot_yaw(framing.yaw)),
    };
    let pixels = offscreen.frame(size, from)?;
    encode(output, &pixels, size)
}

/// Writes tightly packed RGBA8 pixel data to a PNG file.
fn encode(output: &Path, pixels: &[u8], size: (u32, u32)) -> Result<(), String> {
    let file = std::fs::File::create(output)
        .map_err(|err| format!("failed to create {output:?}: {err}"))?;

    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size.0, size.1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = encoder
        .write_header()
        .map_err(|err| format!("failed to write {output:?}: {err}"))?;
    writer
        .write_image_data(pixels)
        .map_err(|err| format!("failed to write {output:?}: {err}"))?;

    Ok(())
}
