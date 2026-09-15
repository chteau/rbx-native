//! GPU presentation of `crate::scene::gui`: the place's `ScreenGui` trees
//! composited over the finished frame, and its `BillboardGui`/`SurfaceGui`
//! canvases placed inside the scene.
//!
//! The screen overlay is the one pass that runs *after* `Post::resolve` (see
//! `Renderer::draw`), straight onto the caller's own display target. That is
//! deliberate: a `ScreenGui` is not part of the scene, so bloom, depth of
//! field and the tone map must not touch it, and drawing it last is what keeps
//! it crisp. The in-world canvases are the opposite — they *are* scene
//! content, so [`space`] draws them before the resolve, depth-tested against
//! the geometry around them.
//!
//! All three paint through one [`paint::Painter`], parameterised by its target
//! and that target's pixel size; the images they sample live in one shared
//! [`atlas::Atlas`]. Everything about a `ScreenGui` in this viewer is static,
//! so its vertex buffer is built once and kept: only a resize invalidates it,
//! since a `UDim2`'s scale half is a fraction of the viewport.

mod atlas;
mod paint;
mod pipeline;
mod quads;
mod space;

use glam::{Mat4, Vec3};

use super::pipeline::Target;
use super::post::Targets;
use crate::quality::QualityProfile;
use crate::scene::{gui_layout, GuiScreen, SpaceGui};
use atlas::Atlas;
use paint::Painter;
use space::Space;

pub(super) struct Gui {
    atlas: Atlas,
    screen: Painter,
    screens: Vec<GuiScreen>,
    /// The viewport the overlay was laid out for; a different one rebuilds it.
    built: Option<(u32, u32)>,
    space: Space,
}

impl Gui {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        target: Target,
        (screens, spaces): (&[GuiScreen], &[SpaceGui]),
        quality: &QualityProfile,
    ) -> Self {
        let enabled = quality.gui;
        let screens: &[GuiScreen] = match enabled {
            true => screens,
            false => &[],
        };
        let spaces: &[SpaceGui] = match enabled {
            true => spaces,
            false => &[],
        };

        // One download and one upload for all three container kinds: both
        // collectors skip an asset already in the list, so the same
        // `ImageLabel` image on a screen and on a surface is fetched once.
        let mut references = Vec::new();
        for screen in screens {
            screen.assets(&mut references);
        }
        for gui in spaces {
            gui.assets(&mut references);
        }
        let atlas = Atlas::new(device, queue, &references, quality);

        let viewport_layout = pipeline::viewport_layout(device);
        let screen_painter = Painter::new(device, format, &viewport_layout, &atlas.image_layout);
        let space = Space::new(device, queue, target, &viewport_layout, &atlas, spaces);

        Gui {
            atlas,
            screen: screen_painter,
            screens: screens.to_vec(),
            built: None,
            space,
        }
    }

    /// Rebuilds the in-world pipelines for a new sample count — see
    /// `renderer::switch`. The overlay's own pipeline draws after the resolve
    /// and so never multisamples.
    pub(super) fn set_target(&mut self, device: &wgpu::Device, target: Target) {
        self.space.set_target(device, target);
    }

    /// Draws every `BillboardGui`/`SurfaceGui` canvas into the scene, before
    /// the resolve and after the passes it has to blend over.
    pub(super) fn draw_space(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        eye: Vec3,
        view_projection: Mat4,
    ) {
        self.space
            .draw(device, queue, encoder, targets, eye, view_projection);
    }

    /// Lays the screens out for `size` if that is new, then paints every
    /// rectangle over `target` in one load-preserving pass.
    pub(super) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        if self.screens.is_empty() {
            return;
        }
        if self.built != Some(size) {
            self.built = Some(size);
            let elements = gui_layout(&self.screens, [size.0 as f32, size.1 as f32]);
            self.screen
                .prepare(device, queue, &elements, self.atlas.slot_of(), size);
        }
        self.screen.draw(
            encoder,
            target,
            wgpu::LoadOp::Load,
            self.atlas.groups(),
            size,
        );
    }
}
