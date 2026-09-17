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
mod gradient;
mod paint;
mod pipeline;
mod quads;
mod space;

use glam::{Mat4, Vec3};

use super::pipeline::Target;
use super::post::Targets;
use crate::load::Answered;
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
    /// Bind group 0's layout for both painters, kept for the canvas painter a
    /// rebuild may still have to build (see `Space::rebuild`).
    viewport_layout: wgpu::BindGroupLayout,
    /// Whether the quality profile draws GUIs at all — `false` keeps every
    /// tree out. Re-read from the profile by every [`Gui::rebuild`], so a
    /// level switched between two scenes takes.
    enabled: bool,
}

impl Gui {
    /// `images` is what the loader decoded for the trees' `ImageLabel`s
    /// (see `Decor::gui`); this pass downloads nothing of its own.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        target: Target,
        (screens, spaces): (&[GuiScreen], &[SpaceGui]),
        images: &Answered,
        quality: &QualityProfile,
    ) -> Self {
        let atlas = Atlas::new(device, queue, &[], images, quality);
        let viewport_layout = pipeline::viewport_layout(device);
        let screen_painter =
            Painter::new(device, queue, format, &viewport_layout, &atlas.image_layout);
        let space = Space::new(device, queue, target, &viewport_layout, &atlas, &[]);

        let mut gui = Gui {
            atlas,
            screen: screen_painter,
            screens: Vec::new(),
            built: None,
            space,
            viewport_layout,
            enabled: quality.gui,
        };
        gui.rebuild(device, queue, (screens, spaces), images, quality);
        gui
    }

    /// Replaces every tree with `screens` and `spaces`, keeping both painters
    /// and every image the atlas already holds (see [`Atlas::extend`]): the
    /// overlay is laid out again on the next frame, the canvases are baked
    /// again here.
    pub(super) fn rebuild(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (screens, spaces): (&[GuiScreen], &[SpaceGui]),
        images: &Answered,
        quality: &QualityProfile,
    ) {
        self.enabled = quality.gui;
        let screens: &[GuiScreen] = match self.enabled {
            true => screens,
            false => &[],
        };
        let spaces: &[SpaceGui] = match self.enabled {
            true => spaces,
            false => &[],
        };

        // One upload for all three container kinds: both collectors skip an
        // asset already in the list, so the same `ImageLabel` image on a
        // screen and on a surface is uploaded once.
        let mut references = Vec::new();
        for screen in screens {
            screen.assets(&mut references);
        }
        for gui in spaces {
            gui.assets(&mut references);
        }
        self.atlas
            .extend(device, queue, &references, images, quality);

        self.screens = screens.to_vec();
        self.built = None;
        self.space
            .rebuild(device, queue, &self.viewport_layout, &self.atlas, spaces);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::QualityLevel;

    // A quality level switched between two scenes has to take on the next
    // rebuild: whether GUIs draw is read from the profile every rebuild, not
    // only once when the pass was built.
    #[test]
    fn a_rebuild_follows_the_quality_toggle_it_is_given() {
        let Some((device, queue)) = crate::gpu::for_tests() else {
            return;
        };
        let target = Target {
            format: crate::renderer::post::HDR_FORMAT,
            samples: 1,
        };
        let mut on = QualityLevel::Automatic.profile();
        on.gui = true;
        let mut off = on;
        off.gui = false;
        let images = Answered::new();
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;

        let mut gui = Gui::new(&device, &queue, format, target, (&[], &[]), &images, &off);
        assert!(!gui.enabled);

        gui.rebuild(&device, &queue, (&[], &[]), &images, &on);
        assert!(gui.enabled);

        gui.rebuild(&device, &queue, (&[], &[]), &images, &off);
        assert!(!gui.enabled);
    }
}
