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
mod group;
mod paint;
mod pipeline;
mod quads;
mod space;
mod text;
mod viewport;

use glam::{Mat4, Vec3};
use rbx_dom::Ref;

use super::pipeline::Target;
use super::post::Targets;
use crate::fonts::Library;
use crate::load::Answered;
use crate::quality::QualityProfile;
use crate::scene::{
    gui_layout_with, gui_scroll_target, GuiScreen, GuiScrollWindow, ScrollTarget, SpaceGui,
};
use atlas::Atlas;
use group::Baked;
use paint::Painter;
use space::Space;
use text::Typesetter;
use viewport::Viewports;

/// One laid-out `GuiObject`: where it came to on screen and how far it is
/// turned — `AbsolutePosition`/`AbsoluteSize`/`AbsoluteRotation`, as an
/// editor hit-tests and draws handles against them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuiBox {
    pub referent: Ref,
    /// `x, y, width, height` in pixels of the frame drawn, before the turn:
    /// the box is rotated by `rotation` degrees clockwise about its centre.
    pub rect: [f32; 4],
    pub rotation: f32,
}

pub(super) struct Gui {
    atlas: Atlas,
    /// The one font system, rasteriser and glyph atlas behind every text
    /// quad, on screen or on a canvas.
    text: Typesetter,
    screen: Painter,
    /// The display format the overlay paints in — and so the format every
    /// `CanvasGroup` is flattened in, since the same painter does both.
    format: wgpu::TextureFormat,
    screens: Vec<GuiScreen>,
    /// The viewport the overlay was laid out for, and which screen it holds
    /// — every enabled one, or the one [`Gui::draw_canvas`] asked for; a
    /// different answer to either rebuilds it.
    built: Option<((u32, u32), Option<Ref>)>,
    /// Every element of the last layout, in paint order — see [`GuiBox`].
    boxes: Vec<GuiBox>,
    /// Every `ScrollingFrame` window of the current overlay, in paint order,
    /// for [`Gui::scroll_target`]. Empty until the first layout.
    windows: Vec<GuiScrollWindow>,
    /// The flattened `CanvasGroup`s of the current overlay, and every
    /// texture its runs can name (the atlas' slots first, then those).
    baked: Baked,
    bindings: Vec<wgpu::BindGroup>,
    space: Space,
    /// Bind group 0's layout for both painters, kept for the canvas painter a
    /// rebuild may still have to build (see `Space::rebuild`).
    viewport_layout: wgpu::BindGroupLayout,
    /// Whether the quality profile draws GUIs at all — `false` keeps every
    /// tree out. Re-read from the profile by every [`Gui::rebuild`], so a
    /// level switched between two scenes takes.
    enabled: bool,
    /// The pass that bakes a `ViewportFrame`'s 3D content into the texture
    /// its quad samples, on screen or on a canvas.
    viewports: Viewports,
}

impl Gui {
    /// `images` is what the loader decoded for the trees' `ImageLabel`s
    /// (see `Decor::gui`) and `fonts` what it fetched for their text; this
    /// pass downloads nothing of its own. `materials` is the renderer's
    /// material arrays, bound with `material_layout`: what a `ViewportFrame`'s
    /// parts are shaded with.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        target: Target,
        (screens, spaces): (&[GuiScreen], &[SpaceGui]),
        (material_layout, materials): (&wgpu::BindGroupLayout, &wgpu::BindGroup),
        images: &Answered,
        fonts: &Library,
        quality: &QualityProfile,
    ) -> Self {
        let mut atlas = Atlas::new(device, queue, &[], images, quality);
        let mut text = Typesetter::new();
        let viewport_layout = pipeline::viewport_layout(device);
        let screen_painter =
            Painter::new(device, queue, format, &viewport_layout, &atlas.image_layout);
        let mut viewports = Viewports::new(device, queue, material_layout, quality);
        let space = Space::new(
            device,
            queue,
            target,
            &viewport_layout,
            (&mut atlas, &mut text, &mut viewports),
            materials,
            &[],
        );

        let mut gui = Gui {
            atlas,
            text,
            screen: screen_painter,
            format,
            screens: Vec::new(),
            built: None,
            boxes: Vec::new(),
            windows: Vec::new(),
            baked: Baked::new(device),
            bindings: Vec::new(),
            space,
            viewport_layout,
            enabled: quality.gui,
            viewports,
        };
        gui.rebuild(
            device,
            queue,
            (screens, spaces),
            materials,
            images,
            fonts,
            quality,
        );
        gui
    }

    /// Replaces every tree with `screens` and `spaces`, keeping both painters
    /// and every image the atlas already holds (see [`Atlas::extend`]): the
    /// overlay is laid out again on the next frame, the canvases are baked
    /// again here.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rebuild(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (screens, spaces): (&[GuiScreen], &[SpaceGui]),
        materials: &wgpu::BindGroup,
        images: &Answered,
        fonts: &Library,
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
        // Same for the faces: a face the loader has since landed is loaded
        // once, and the overlay laid out below is shaped with it.
        let mut faces = Vec::new();
        for screen in screens {
            screen.fonts(&mut faces);
        }
        for gui in spaces {
            gui.fonts(&mut faces);
        }
        self.text.adopt(fonts, &faces);

        self.screens = screens.to_vec();
        self.built = None;
        // The wheel's hit list is deliberately left standing: the draw below
        // replaces it wholesale, and until then one layout's worth of stale
        // windows is far better than none. A scroll writes `CanvasPosition`
        // and comes straight back here as a change, so clearing it would
        // drop every notch of a fast scroll that arrived in the same batch
        // — and a dropped notch is not nothing, it is the camera zooming.
        self.space.rebuild(
            device,
            queue,
            &self.viewport_layout,
            (&mut self.atlas, &mut self.text, &mut self.viewports),
            materials,
            spaces,
        );
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

    /// Lays the screens out for `size` if that is new — baking every
    /// `ViewportFrame` at the pixel size it came to, which is why
    /// `materials` is needed here — then paints every rectangle over
    /// `target` in one load-preserving pass.
    ///
    /// The texture, not a view of it: the overlay composites in encoded space
    /// and so attaches its own non-sRGB view (see `pipeline::encoded`).
    pub(super) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        size: (u32, u32),
        materials: &wgpu::BindGroup,
    ) {
        if !self.screens.iter().any(|screen| screen.enabled) {
            // Nothing to draw and nothing to scroll: the overlay is gone,
            // so the windows `rebuild` left standing have to go with it.
            self.windows.clear();
            return;
        }
        self.lay_out(device, queue, size, None, materials);
        self.screen.draw(
            encoder,
            &pipeline::encoded_view(target),
            wgpu::LoadOp::Load,
            &self.bindings,
            size,
        );
    }

    /// One `ScreenGui` on its own over a flat `backdrop` — an editor's
    /// canvas: enabled or not, laid out against `size` as if that were the
    /// whole screen, with nothing of the scene under it. The same layout
    /// and painter the overlay uses, so the canvas cannot draw a tree any
    /// differently from the viewport.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_canvas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        (size, only): ((u32, u32), Ref),
        backdrop: wgpu::Color,
        materials: &wgpu::BindGroup,
    ) {
        self.lay_out(device, queue, size, Some(only), materials);
        self.screen.draw(
            encoder,
            &pipeline::encoded_view(target),
            wgpu::LoadOp::Clear(backdrop),
            &self.bindings,
            size,
        );
    }

    /// Every element of the last layout, in paint order.
    pub(super) fn boxes(&self) -> &[GuiBox] {
        &self.boxes
    }

    /// Lays out the enabled screens — or `only` that one, enabled or not —
    /// for `size` if that is not what is already built.
    fn lay_out(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        only: Option<Ref>,
        materials: &wgpu::BindGroup,
    ) {
        if self.built != Some((size, only)) {
            self.built = Some((size, only));
            let screens = self.screens.iter().filter(|screen| match only {
                Some(referent) => screen.referent == referent,
                None => screen.enabled,
            });
            let mut elements =
                gui_layout_with(screens, [size.0 as f32, size.1 as f32], &mut self.text);
            // Before the flatten below as well, for the same reason: a
            // `CanvasGroup`'s children are still things to click on.
            self.boxes = elements
                .iter()
                .map(|element| GuiBox {
                    referent: element.referent,
                    rect: [
                        element.rect.x,
                        element.rect.y,
                        element.rect.width,
                        element.rect.height,
                    ],
                    rotation: element.rotation,
                })
                .collect();
            // Before the flatten below, which folds a `CanvasGroup`'s subtree
            // away: a list inside a group still scrolls.
            self.windows = elements
                .iter()
                .filter_map(|element| element.scroll)
                .collect();
            self.viewports.bake_all(
                device,
                queue,
                materials,
                &mut self.atlas,
                "screen",
                &mut elements,
            );
            self.baked.clear();
            let elements = group::flatten(
                &mut group::Bake {
                    device,
                    queue,
                    painter: &mut self.screen,
                    atlas: &mut self.atlas,
                    fonts: &mut self.text,
                    format: self.format,
                    baked: &mut self.baked,
                },
                elements,
            );
            self.screen.prepare(
                device,
                queue,
                &elements,
                self.atlas.slot_of(),
                size,
                &mut self.text,
            );
            self.atlas.sync_glyphs(device, queue, &mut self.text.atlas);
            self.bindings = self.baked.bindings(&self.atlas);
        }
    }

    /// The `ScrollingFrame` the wheel over `point` would scroll along `axis`,
    /// against the overlay as last laid out — `None` before the first draw,
    /// and until the draw after a rebuild.
    pub(super) fn scroll_target(&self, point: [f32; 2], axis: usize) -> Option<ScrollTarget> {
        gui_scroll_target(&self.windows, point, axis)
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{UDim, UDim2, Variant, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    use super::*;
    use crate::quality::QualityLevel;
    use crate::scene::{gui_plan, Catalog};

    fn udim2(ox: i32, oy: i32) -> Variant {
        Variant::UDim2(UDim2 {
            x: UDim {
                scale: 0.0,
                offset: ox,
            },
            y: UDim {
                scale: 0.0,
                offset: oy,
            },
        })
    }

    /// A `ScreenGui` holding one 200 × 100 `ScrollingFrame` at the origin
    /// with a canvas of `canvas` pixels.
    fn place(canvas: (i32, i32)) -> (WeakDom, rbx_dom::Ref) {
        let mut dom = WeakDom::new();
        let gui = dom.new_instance("ScreenGui", "ScreenGui", None);
        dom.set_property(gui, "ScreenInsets", Variant::Enum(0))
            .unwrap();
        let frame = dom.new_instance("ScrollingFrame", "List", Some(gui));
        dom.set_property(frame, "Size", udim2(200, 100)).unwrap();
        dom.set_property(frame, "CanvasSize", udim2(canvas.0, canvas.1))
            .unwrap();
        dom.set_property(frame, "ScrollBarThickness", Variant::Int32(12))
            .unwrap();
        (dom, frame)
    }

    fn screens(dom: &WeakDom) -> Vec<GuiScreen> {
        let database = ReflectionDatabase::embedded();
        gui_plan(dom, &database, &mut Catalog::new(dom, &database))
    }

    // The wheel's hit list is a by-product of the overlay's own layout: it
    // exists once the overlay has been drawn, and follows a rebuild — a
    // canvas that has since come to fit its window is no longer a target.
    #[test]
    fn the_scroll_targets_follow_the_overlay_as_drawn() {
        let Some((device, queue)) = crate::gpu::for_tests() else {
            return;
        };
        let target = Target {
            format: crate::renderer::post::HDR_FORMAT,
            samples: 1,
        };
        let mut quality = QualityLevel::Automatic.profile();
        quality.gui = true;
        let images = Answered::new();
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let fonts = Library::default();
        let material_layout = crate::renderer::material::layout(&device);
        let materials = crate::renderer::material::Materials::new(
            &device,
            &queue,
            &material_layout,
            &Catalog::new(&WeakDom::new(), &ReflectionDatabase::embedded()),
            &quality,
        );
        let (dom, frame) = place((200, 300));
        let mut gui = Gui::new(
            &device,
            &queue,
            format,
            target,
            (&screens(&dom), &[]),
            (&material_layout, &materials.bind_group),
            &images,
            &fonts,
            &quality,
        );
        assert!(
            gui.scroll_target([50.0, 50.0], 1).is_none(),
            "not laid out yet"
        );

        let size = (400, 300);
        let display = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[pipeline::encoded(format)],
        });
        let draw = |gui: &mut Gui| {
            let mut encoder = device.create_command_encoder(&Default::default());
            gui.draw(
                &device,
                &queue,
                &mut encoder,
                &display,
                size,
                &materials.bind_group,
            );
            queue.submit(std::iter::once(encoder.finish()));
        };

        draw(&mut gui);
        let hit = gui.scroll_target([50.0, 50.0], 1).unwrap();
        assert_eq!(hit.referent, frame);
        assert_eq!(hit.range, 200.0);
        assert!(gui.scroll_target([50.0, 50.0], 0).is_none(), "fits across");
        assert!(gui.scroll_target([250.0, 50.0], 1).is_none(), "beside it");

        let (fitted, _) = place((200, 100));
        gui.rebuild(
            &device,
            &queue,
            (&screens(&fitted), &[]),
            &materials.bind_group,
            &images,
            &fonts,
            &quality,
        );
        // Standing until the draw that replaces them, not cleared: a scroll
        // writes `CanvasPosition`, which comes back as a rebuild, and the
        // next notch of the same flick must still find the frame.
        assert!(
            gui.scroll_target([50.0, 50.0], 1).is_some(),
            "stale, not gone"
        );
        draw(&mut gui);
        assert!(gui.scroll_target([50.0, 50.0], 1).is_none());

        // An overlay that goes away takes them with it, since a screenless
        // draw has no layout to replace them with.
        gui.rebuild(
            &device,
            &queue,
            (&screens(&dom), &[]),
            &materials.bind_group,
            &images,
            &fonts,
            &quality,
        );
        draw(&mut gui);
        assert!(gui.scroll_target([50.0, 50.0], 1).is_some());
        gui.rebuild(
            &device,
            &queue,
            (&[], &[]),
            &materials.bind_group,
            &images,
            &fonts,
            &quality,
        );
        draw(&mut gui);
        assert!(gui.scroll_target([50.0, 50.0], 1).is_none());
    }

    // The canvas is one screen on its own, disabled or not; the overlay is
    // every enabled screen and never a disabled one. Both are read back off
    // the boxes the layout leaves behind.
    #[test]
    fn the_canvas_lays_out_one_screen_and_the_overlay_only_enabled_ones() {
        let Some((device, queue)) = crate::gpu::for_tests() else {
            return;
        };
        let (mut dom, shown) = place((200, 100));
        let hidden = dom.new_instance("ScreenGui", "Hidden", None);
        dom.set_property(hidden, "Enabled", Variant::Bool(false))
            .unwrap();
        dom.set_property(hidden, "ScreenInsets", Variant::Enum(0))
            .unwrap();
        let menu = dom.new_instance("Frame", "Menu", Some(hidden));
        dom.set_property(menu, "Size", udim2(50, 40)).unwrap();

        let target = Target {
            format: crate::renderer::post::HDR_FORMAT,
            samples: 1,
        };
        let mut quality = QualityLevel::Automatic.profile();
        quality.gui = true;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let material_layout = crate::renderer::material::layout(&device);
        let materials = crate::renderer::material::Materials::new(
            &device,
            &queue,
            &material_layout,
            &Catalog::new(&WeakDom::new(), &ReflectionDatabase::embedded()),
            &quality,
        );
        let mut gui = Gui::new(
            &device,
            &queue,
            format,
            target,
            (&screens(&dom), &[]),
            (&material_layout, &materials.bind_group),
            &Answered::new(),
            &Library::default(),
            &quality,
        );
        let size = (400, 300);
        let display = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[pipeline::encoded(format)],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let referents = |gui: &Gui| -> Vec<rbx_dom::Ref> {
            gui.boxes().iter().map(|placed| placed.referent).collect()
        };

        gui.draw(
            &device,
            &queue,
            &mut encoder,
            &display,
            size,
            &materials.bind_group,
        );
        assert!(referents(&gui).contains(&shown));
        assert!(!referents(&gui).contains(&menu), "disabled, so not drawn");

        gui.draw_canvas(
            &device,
            &queue,
            &mut encoder,
            &display,
            (size, hidden),
            wgpu::Color::BLACK,
            &materials.bind_group,
        );
        assert_eq!(referents(&gui), [menu], "that screen alone");
        assert_eq!(gui.boxes()[0].rect, [0.0, 0.0, 50.0, 40.0]);
        queue.submit(std::iter::once(encoder.finish()));
    }

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

        let fonts = Library::default();
        let material_layout = crate::renderer::material::layout(&device);
        let materials = crate::renderer::material::Materials::new(
            &device,
            &queue,
            &material_layout,
            &crate::scene::Catalog::new(&rbx_dom::WeakDom::new(), &ReflectionDatabase::embedded()),
            &on,
        );
        let mut gui = Gui::new(
            &device,
            &queue,
            format,
            target,
            (&[], &[]),
            (&material_layout, &materials.bind_group),
            &images,
            &fonts,
            &off,
        );
        assert!(!gui.enabled);

        gui.rebuild(
            &device,
            &queue,
            (&[], &[]),
            &materials.bind_group,
            &images,
            &fonts,
            &on,
        );
        assert!(gui.enabled);

        gui.rebuild(
            &device,
            &queue,
            (&[], &[]),
            &materials.bind_group,
            &images,
            &fonts,
            &off,
        );
        assert!(!gui.enabled);
    }
}
