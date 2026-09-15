//! Offscreen path: renders frames into a texture the GPU reads straight back,
//! either written out as a PNG or handed to an embedder (see [`crate::headless`]).

mod readback;

use std::path::Path;
use std::time::{Duration, Instant};

use glam::Vec3;
use rbx_dom::Ref;

use crate::camera::{self, Camera, Viewpoint};
use crate::gpu;
use crate::lighting::{Lighting, LocalLight};
use crate::quality::QualityProfile;
use crate::renderer::{Renderer, World};
use crate::scene::{EffectKind, Part, ResolvedInstance, Scene};
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
}

impl Offscreen {
    pub(crate) fn new(world: World<'_>, quality: &QualityProfile) -> Result<Self, String> {
        let instance = gpu::instance();
        let adapter = gpu::adapter(&instance, None)?;
        let (device, queue) = gpu::device(&adapter)?;
        let renderer = Renderer::new(&device, &queue, FORMAT, world, quality);

        Ok(Offscreen {
            device,
            queue,
            renderer,
            target: None,
            pending: None,
        })
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

    /// Replaces the outlined selection box(es), rebuilding their tiny vertex
    /// buffer right away.
    pub(crate) fn set_selection(&mut self, referents: &[Ref]) {
        self.renderer.set_selection(&self.device, referents);
    }

    /// Forwards to [`Renderer::update_lighting`] — see its doc comment for
    /// what does and does not need a GPU write here.
    pub(crate) fn update_lighting(&mut self, lighting: Lighting, lights: &[LocalLight]) -> bool {
        self.renderer.update_lighting(&self.queue, lighting, lights)
    }

    /// Forwards to [`Renderer::patch_instance`].
    pub(crate) fn patch_instance(&mut self, part: &Part) -> bool {
        self.renderer.patch_instance(&self.queue, part)
    }

    /// Forwards to [`Renderer::patch_mesh_instance`].
    pub(crate) fn patch_mesh_instance(&mut self, instance: &ResolvedInstance) -> bool {
        self.renderer.patch_mesh_instance(&self.queue, instance)
    }

    /// Forwards to [`Renderer::patch_effect`].
    pub(crate) fn patch_effect(&mut self, kind: EffectKind, scene: &Scene) -> bool {
        self.renderer.patch_effect(kind, scene)
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
            .draw(&self.device, &self.queue, target.view(), size, from);
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

    /// Drops the queued frame, mapping and all: a readback buffer left mapped is
    /// one the next copy into it cannot use.
    fn discard(&mut self) {
        if let (Some(pending), Some(target)) = (self.pending.take(), &self.target) {
            let _ = target.collect(&self.device, pending);
        }
    }
}

/// Renders a single frame offscreen and writes it as PNG.
///
/// `eye_look_at`, when given, places the camera directly instead of framing the
/// scene's bounds — the only way to get close enough to examine a small prop in a
/// huge map — and takes over from `yaw`/`pitch` entirely, matching how the free
/// camera's pose already overrides the orbit camera in the windowed path.
pub(crate) fn write_png(
    world: World<'_>,
    quality: &QualityProfile,
    output: &Path,
    size: (u32, u32),
    yaw: Option<f32>,
    pitch: Option<f32>,
    eye_look_at: Option<(Vec3, Vec3)>,
) -> Result<(), String> {
    let mut offscreen = Offscreen::new(world, quality)?;

    if let Some(degrees) = pitch {
        offscreen.pitch(degrees);
    }

    let from = match eye_look_at {
        Some((eye, look_at)) => Viewpoint::Free(camera::look_at_pose(eye, look_at)),
        None => Viewpoint::Orbit(Camera::screenshot_yaw(yaw)),
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
