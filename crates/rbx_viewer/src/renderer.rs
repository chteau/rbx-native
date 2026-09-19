//! Draws a scene's parts, the decals projected onto them, its sky and its
//! celestial bodies into any render target, window or texture alike.

mod beam;
mod cull;
mod envmap;
mod filemesh;
mod geometry;
mod gizmo;
mod gui;
mod hover;
mod instance;
mod lighting;
mod material;
mod mesh;
mod outline;
mod particles;
mod pass;
mod patch;
mod pipeline;
mod post;
mod rebuild;
mod selection;
mod shadow;
mod shaped;
mod skybox;
mod slots;
mod stars;
mod sun;
mod switch;
mod texture;
mod textured;
mod trail;
mod translucent;

use glam::Mat3;

use crate::camera::{Camera, Frustum, Viewpoint};
use crate::fonts::Library;
use crate::gizmo::{arm_length, basis, Faces, Gizmo, Handles, Kind, Shape};
use crate::lighting::{Lighting, LocalLight};
use crate::load::Answered;
use crate::pick::Selected;
use crate::quality::QualityProfile;
use crate::scene::{Bounds, Scene, ScrollTarget};
use crate::textures::Decor;
use beam::Beams;
use cull::MainCull;
use envmap::EnvMap;
use geometry::Meshes;
use gizmo::Draggers;
use gui::Gui;
use hover::Hover;
use lighting::LightingRaw;
use material::Materials;
use particles::Particles;
use pipeline::{Frame, Shared, Target};
use post::Post;
use selection::Selection;
use shadow::{Fit, Lamp, Shadows};
use shaped::Shaped;
use stars::Stars;
use texture::PER_FRAME;
use trail::Trails;
use translucent::Translucent;

/// Only ever seen where a scene has no `Sky`, or where one of its six panels
/// would not resolve.
const CLEAR_COLOR: wgpu::Color = wgpu::Color::BLACK;

/// Everything a frame is drawn from. They always travel together — both
/// render paths read the same DOM once and hand the lot straight on — so they
/// move as one argument rather than five.
#[derive(Clone, Copy)]
pub(crate) struct World<'a> {
    pub(crate) scene: &'a Scene,
    pub(crate) decor: &'a Decor,
    pub(crate) lighting: &'a Lighting,
    /// The place's `PointLight`s, `SpotLight`s and `SurfaceLight`s. Empty with
    /// `--no-lights`. Uploaded once here; an edit that moves, adds or removes
    /// one rewrites them through `Renderer::set_lights`.
    pub(crate) lights: &'a [LocalLight],
    /// Every image the passes below fetch for themselves — the `Beam`,
    /// `Trail` and `ParticleEmitter` textures and the GUI atlas — as far as
    /// the loader has an answer for them. Handed in rather than resolved here
    /// because resolving one is a download and a decode, and this is the
    /// thread that draws: see `load::Answered`, whose "no answer yet" is what
    /// keeps a pass from writing an effect off before its texture lands.
    pub(crate) images: &'a Answered,
    /// Every font face the GUI trees' text needs, as far as the loader has
    /// it — see `load::Loaded::resolve_fonts`. Same reasoning as `images`.
    pub(crate) fonts: &'a Library,
}

/// Coordinates rendering to any target (window surface or offscreen texture).
///
/// Maintains the GPU state: the unit meshes, per-part transforms and colors, the
/// projected decals, the sky, the lighting uniform and the depth buffer. The same
/// Renderer is shared between windowed and offscreen paths.
pub(crate) struct Renderer {
    opaque: wgpu::RenderPipeline,
    blended: wgpu::RenderPipeline,
    frame: Frame,
    /// Bind group 0's layout and bind group 1's, kept for the life of the
    /// renderer: a change of quality level rebuilds those groups (see
    /// [`Renderer::set_quality`]) around re-viewed textures.
    frame_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    lighting: Lighting,
    lighting_buffer: wgpu::Buffer,
    /// Every local light the place has, and the buffer holding the prefix of them
    /// the level allows. Kept so a level with a different cap can rewrite the
    /// buffer without the place being read again.
    all_lights: Vec<LocalLight>,
    lights_buffer: wgpu::Buffer,
    /// How many entries of the local light buffer the shader has to read.
    lights: usize,
    /// One record per entry of `lights_buffer`, rewritten every frame with
    /// whichever of them `shadow::local::select` chose (see
    /// [`Renderer::draw`]). Always as long as `lights_buffer`, which is what
    /// keeps the two arrays in step in `lights.wgsl`.
    light_shadows_buffer: wgpu::Buffer,
    /// The sky, prefiltered: the cube the environment terms sample, its mip depth
    /// and the irradiance the diffuse term rebuilds from.
    env: EnvMap,
    meshes: Meshes,
    materials: Materials,
    shaped: Shaped,
    translucent: Translucent,
    filemesh: filemesh::FileMeshes,
    textured: textured::Textured,
    sky: Option<skybox::Skybox>,
    stars: Option<Stars>,
    bodies: Option<sun::Bodies>,
    shadows: Shadows,
    /// `Beam` ribbons, drawn after opaque geometry and before particles — see
    /// [`Renderer::draw`].
    beams: Beams,
    /// `Trail` ribbons, drawn right after beams — see [`Renderer::draw`].
    trails: Trails,
    /// Billboarded `ParticleEmitter`s, drawn after everything above — see
    /// [`Renderer::draw`].
    particles: Particles,
    /// The Explorer's selection outline. Reads `self.frame`'s bind group at
    /// draw time, so it needs no camera state of its own.
    selection: Selection,
    /// The "about to click" cue drawn around whatever `BasePart` the cursor is
    /// over, distinctly from `selection` above — see `renderer::hover`. Reads
    /// the same shared bind group, for the same reason.
    hover: Hover,
    /// The transform tool's axis draggers, drawn over the selection outline.
    draggers: Draggers,
    /// Which transform tool the editor has active, if any — `None` while the
    /// Select tool is, which is also every `rbxview` frame (the standalone
    /// viewer edits nothing).
    gizmo: Option<Gizmo>,
    /// The place's GUI containers: the `ScreenGui` overlay, drawn last of all
    /// straight onto the display target so no post effect touches it, and the
    /// `BillboardGui`/`SurfaceGui` canvases, drawn inside the scene.
    gui: Gui,
    camera: Camera,
    /// The scene's extent, which is what the shadow map's depth range and its
    /// recentring are fitted against every frame.
    bounds: Bounds,
    /// The HDR target every pass above draws into, its depth buffer, and what
    /// turns the two back into an image the caller can present.
    post: Post,
    /// The graphics quality level the next frame is drawn at.
    /// [`Renderer::set_quality`] moves it without rebuilding the scene.
    quality: QualityProfile,
    /// What every surface pipeline above was built for: the HDR format, and the
    /// profile's `msaa_samples` as far as the adapter allows (see `post`).
    target: Target,
}

impl Renderer {
    /// Builds GPU state from the scene, its decor and its lighting: the unit
    /// meshes both passes instance, an instance buffer per shape, one decal batch
    /// per image and shape, the sky panels and the environment probe built from
    /// them.
    ///
    /// Once, per device: another scene on the same device goes through
    /// [`Renderer::rebuild`], which redoes only the scene-derived half of
    /// this and mirrors it step for step.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        world: World<'_>,
        quality: &QualityProfile,
    ) -> Self {
        let World {
            scene,
            decor,
            lighting,
            lights,
            images,
            fonts,
        } = world;
        // Every scene pass draws into the HDR target rather than into the
        // caller's: only the resolve at the end of `draw` knows `format`.
        let post = Post::new(device, format, lighting.effects, quality);
        // Every surface pipeline below is built for this sample count, so a level
        // that changes it is the one switch that has to rebuild them.
        let target = Target {
            format: post::HDR_FORMAT,
            samples: post.samples(),
        };
        let layout = pipeline::frame_layout(device);
        let lighting_buffer = pipeline::lighting_buffer(device);
        // Capped here rather than at draw time: a light the level does not allow
        // is never uploaded at all, so it costs neither memory nor a loop step.
        let allowed = lights.len().min(quality.local_lights_max);
        let lights_buffer = lighting::local_lights_buffer(device, &lights[..allowed]);
        // No light is selected before the first frame, so every entry starts
        // unshadowed; `Renderer::draw` rewrites the whole buffer every frame
        // after this one anyway.
        let light_shadows_buffer = shadow::local::buffer(device, allowed);
        // The probe is the sky, prefiltered: every pass binds it at group 0, so
        // it exists (as one grey texel at worst) even without a `Sky`.
        let env = EnvMap::new(device, queue, decor.sky.as_deref(), quality);
        // Decals are projected on the parts' own meshes, so every shape they use
        // is already among the scene's own — no extra kind can appear here.
        let meshes = Meshes::new(device, shaped::kinds(scene.parts()));
        let material_layout = material::layout(device);
        let (opaque, blended) =
            pipeline::shape_pipelines(device, target, &layout, &material_layout);

        let shadows = Shadows::new(device, scene, quality);
        let shared = Shared {
            lighting: &lighting_buffer,
            lights: &lights_buffer,
            env: &env,
            shadow_map: shadows.view(),
            shadow_sampler: shadows.sampler(),
            local_shadow_map: shadows.local_view(),
            light_shadows: &light_shadows_buffer,
        };
        let frame = Frame::new(device, &layout, shared);
        let sky = decor.sky.as_ref().map(|panels| {
            skybox::Skybox::new(device, queue, target, &layout, shared, panels, quality)
        });
        let bodies = sun::Bodies::new(
            device,
            queue,
            target,
            &layout,
            shared,
            &decor.bodies,
            quality,
        );
        let stars = Stars::new(device, target, &layout, shared, &decor.stars);
        // Read once here rather than kept as a whole `Scene`: an edit keeps
        // the copy in step one placement at a time (see
        // `Renderer::sync_part`), and `Renderer::new` has no other reason
        // to hold on to the scene itself. `hover` keeps its own copy rather
        // than sharing `selection`'s: the two outlines' GPU state stays
        // independent, at the cost of one extra clone paid once here.
        let placements = scene.all_placements();
        let selection = Selection::new(device, target, &layout, placements.clone());
        let hover = Hover::new(device, target, &layout, placements);
        let draggers = Draggers::new(device, target, &layout);
        // Before the GUI pass, which shades a `ViewportFrame`'s parts with
        // the very same arrays.
        let materials = Materials::new(device, queue, &material_layout, scene.materials(), quality);

        Renderer {
            opaque,
            blended,
            frame,
            lighting: *lighting,
            all_lights: lights.to_vec(),
            lights: allowed,
            env,
            meshes,
            shaped: Shaped::new(device, scene.parts()),
            translucent: Translucent::new(device, scene.parts()),
            filemesh: filemesh::FileMeshes::new(
                device,
                queue,
                target,
                &layout,
                &material_layout,
                scene.resolved_file_meshes(),
                quality,
            ),
            textured: textured::Textured::new(
                device,
                queue,
                target,
                &layout,
                &decor.groups,
                quality,
            ),
            sky,
            stars,
            bodies,
            shadows,
            beams: Beams::new(device, queue, target, scene.beams(), images, quality),
            trails: Trails::new(device, queue, target, scene.trails(), images, quality),
            particles: Particles::new(
                device,
                queue,
                target,
                scene.particle_emitters(),
                images,
                quality,
            ),
            selection,
            hover,
            draggers,
            gizmo: None,
            gui: Gui::new(
                device,
                queue,
                format,
                target,
                (scene.gui_screens(), scene.gui_spaces()),
                (&material_layout, &materials.bind_group),
                images,
                fonts,
                quality,
            ),
            materials,
            lighting_buffer,
            lights_buffer,
            light_shadows_buffer,
            frame_layout: layout,
            material_layout,
            camera: Camera::framing(scene.bounds()),
            bounds: *scene.bounds(),
            post,
            quality: *quality,
            target,
        }
    }

    /// Only the offscreen path uses this: the window orbits at the default height.
    pub(crate) fn pitch(&mut self, degrees: f32) {
        self.camera = self.camera.pitched(degrees);
    }

    /// Swaps the main camera between perspective and orthographic projection —
    /// see `Camera::with_orthographic`.
    pub(crate) fn set_orthographic(&mut self, orthographic: bool) {
        self.camera = self.camera.with_orthographic(orthographic);
    }

    /// Replaces the outlined selection, rebuilding its tiny vertex buffer right
    /// away rather than waiting for the next `draw`.
    pub(crate) fn set_selection(&mut self, device: &wgpu::Device, selected: &[Selected]) {
        self.selection.set(device, selected);
    }

    /// Whether scene geometry in front of the selection hides its outline —
    /// see `renderer::selection`, which draws it through everything by
    /// default.
    pub(crate) fn set_selection_occluded(&mut self, occluded: bool) {
        self.selection.set_occluded(occluded);
    }

    /// Replaces the hover outline, rebuilding its tiny vertex buffer right
    /// away rather than waiting for the next `draw`. `None` clears it.
    pub(crate) fn set_hover(&mut self, device: &wgpu::Device, selected: Vec<Selected>) {
        self.hover.set(device, selected);
    }

    /// Shows or hides the transform tool's draggers over whatever is
    /// selected. Their geometry is rebuilt inside [`Renderer::draw`] rather
    /// than here: the arms are scaled to hold a constant size on screen, so
    /// they change with every camera move, not only when the tool does.
    pub(crate) fn set_gizmo(&mut self, gizmo: Option<Gizmo>) {
        self.gizmo = gizmo;
    }

    /// The `ScrollingFrame` of the screen overlay the wheel over `point` (in
    /// pixels of the last frame drawn) would scroll along `axis` — see
    /// `scene::gui::scroll_target`.
    pub(crate) fn gui_scroll_target(&self, point: [f32; 2], axis: usize) -> Option<ScrollTarget> {
        self.gui.scroll_target(point, axis)
    }

    /// Where this frame's draggers sit, or `None` when no transform tool is
    /// active, nothing with a placement is selected, or the camera is still
    /// on its automatic orbit — which only happens in a file with no saved
    /// camera of its own, before the first input, where there is nothing to
    /// drag with yet either.
    fn handles(&self, from: Viewpoint) -> Option<Shape> {
        let gizmo = self.gizmo?;
        let Viewpoint::Free(pose) = from else {
            return None;
        };
        let model = self.selection.anchor()?;
        let orthographic = self.camera.is_orthographic();
        // Scale's balls are bound to the surface of the box the selection
        // scales on — a lone part's own, a group's world-aligned bounds — so
        // there is no origin or arm to pick for them at all.
        if gizmo.kind == Kind::Scale {
            let scaled = self.selection.scale_box().unwrap_or(model);
            return Some(Shape::Scale(Faces::new(scaled, pose, orthographic)));
        }

        let (anchor, rotation) = (model.w_axis.truncate(), Mat3::from_mat4(model));
        // Move drags every selected part by one offset and Rotate turns them
        // all about one point, so both gizmos belong at the middle of the
        // whole selection rather than hanging off whichever part happens to
        // be first (for one part the two are the same place). The *basis*
        // still comes from the anchor: a selection has no aggregate rotation
        // to take.
        let origin = self.selection.centre().unwrap_or(anchor);
        let handles = Handles::new(
            origin,
            basis(gizmo.local.then_some(rotation)),
            arm_length(origin, pose, orthographic),
        );
        Some(match gizmo.kind {
            Kind::Rotate => Shape::Rotate(handles),
            _ => Shape::Move(handles),
        })
    }

    /// Uploads every texture still queued from the last load or reload, all
    /// at once, instead of a bounded amount per [`Renderer::draw`] call.
    ///
    /// For a caller with no next frame to spread the rest across — the
    /// single-shot `rbxview --screenshot` path — rather than one that
    /// forever draws whatever loaded so far, the way a live window or an
    /// embedder's continuously redrawn viewport does.
    pub(crate) fn finish_loading(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.textured
            .upload_pending(device, queue, &self.quality, usize::MAX);
    }

    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        size: (u32, u32),
        from: Viewpoint,
    ) {
        // A minimized window reports a zero-sized surface, which no texture can match.
        if size.0 == 0 || size.1 == 0 {
            return;
        }

        // Before any pass, the sun map's included: an edit since the last
        // frame left its instance records owed to the batch buffers (see
        // `Renderer::flush_writes`).
        self.flush_writes(queue);

        // Bounded so a place with many `Decal`/`Texture` images spreads their
        // GPU upload across the frames after load instead of stalling this
        // one uploading all of them — see `textured::Textured::upload_pending`.
        self.textured
            .upload_pending(device, queue, &self.quality, PER_FRAME);
        // Once here rather than once per moved part: a drag of a whole model
        // reports every one of its parts through `sync_instance` before the
        // frame they all belong to (see `selection::Outline::take_vertices`).
        self.selection.flush(device);

        let aspect = size.0 as f32 / size.1 as f32;
        let eye = self.camera.eye_position(from);
        let view_projection = self.camera.view_projection(from, aspect);
        let viewport = glam::Vec2::new(size.0 as f32, size.1 as f32);
        self.frame.write(queue, &view_projection, viewport);
        // The main pass's own visibility test: tight to the camera's frustum
        // and this level's render distance. The shadow pass below never uses
        // this — see `Fit::visible` — so a caster it culls can still land a
        // shadow inside the frame.
        self.draggers.update(queue, self.handles(from), eye);
        let frustum = Frustum::new(&self.camera, from, aspect);
        let cull = MainCull::new(&frustum, eye, self.quality.render_distance);
        let (lamp, fit) = self.sun_shadow(from, aspect);
        // Specular, reflections and fog all need the eye, so the lighting
        // uniform changes every frame even though the place's `Lighting` does not.
        queue.write_buffer(
            &self.lighting_buffer,
            0,
            bytemuck::bytes_of(&LightingRaw::new(
                &self.lighting,
                eye,
                &self.env.probe,
                (lamp, &fit),
                self.lights,
                &self.quality,
            )),
        );

        // Recomputed every frame: nothing here moves, but the camera does, and
        // it is the lights nearest *it* that earn a map (see
        // `shadow::local::select`). Written whether or not any light was
        // selected, so a level whose cap just dropped clears out last frame's.
        let selected = shadow::local::select(
            &self.all_lights[..self.lights],
            eye,
            self.shadows.local_cap(),
        );
        queue.write_buffer(
            &self.light_shadows_buffer,
            0,
            bytemuck::cast_slice(&shadow::local::pack(self.lights, &selected)),
        );

        let rotation_only = self.camera.view_rotation_projection(from, aspect);
        if let Some(sky) = &self.sky {
            sky.camera.write(queue, &rotation_only, viewport);
        }
        if let Some(stars) = &self.stars {
            stars.camera.write(queue, &rotation_only, viewport);
        }
        if let Some(bodies) = &self.bodies {
            bodies.camera.write(queue, &rotation_only, viewport);
        }
        // The same matrix the sun disc itself is drawn with, so the god-rays
        // in the resolve can never point anywhere the disc is not.
        let sun_screen = sun::sun_screen_position(self.lighting.sun_direction, &rotation_only);

        // Both re-sort and re-upload their instances; neither may run inside the
        // render pass that reads those buffers.
        self.translucent.prepare(queue, eye, &cull);
        self.filemesh.prepare(queue, eye);

        let orthographic = self.camera.orthographic_range(from);
        if self
            .post
            .prepare(device, queue, size, sun_screen, orthographic)
            .is_none()
        {
            return;
        }
        let Some(targets) = self.post.targets() else {
            return;
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rbxview frame"),
        });
        // Before the colour pass, never inside it: the same texture cannot be a
        // depth attachment and a bound resource in one pass.
        if lamp != Lamp::None {
            self.shadows.render(queue, &mut encoder, &self.meshes, &fit);
        }
        if !selected.is_empty() {
            self.shadows
                .render_local(queue, &mut encoder, &self.meshes, &selected);
        }
        self.scene_pass(&mut encoder, targets, &cull);

        // Before particles: sorting the two passes against each other is out
        // of scope for v1 (see `renderer::beam`'s docs), so beams simply go
        // first, both depth-tested against the opaque pass above.
        // What a `LightInfluence`-1 beam is tinted by: a normal-less ribbon
        // has no face to catch the sun, so the flat, average-orientation
        // illumination — the ambient hemisphere plus half of each directional
        // lamp — stands in for "the light in the scene", dimming a beam at
        // night and leaving it near full brightness in daylight. Roblox
        // publishes no beam-lighting formula, so this is a documented
        // approximation rather than a match.
        let l = self.lighting;
        let env_light = l.ambient + 0.5 * (l.sun_color + l.fill_color);
        self.beams.draw(
            queue,
            device,
            &mut encoder,
            targets,
            eye,
            view_projection,
            env_light,
        );
        // Right after beams, same reasoning: sorting the two ribbon passes
        // against each other is out of scope for v1 — see `renderer::trail`'s
        // docs.
        self.trails
            .draw(queue, device, &mut encoder, targets, eye, view_projection);
        // After opaque geometry, depth-tested against it and re-sorted every
        // frame — see `renderer::particles`.
        self.particles
            .draw(queue, device, &mut encoder, targets, eye, view_projection);

        // Last of the scene passes, so a canvas blends over the ribbons and
        // particles as well as over opaque geometry — unlike a `ScreenGui`, a
        // `BillboardGui`/`SurfaceGui` *is* scene content and is tone mapped
        // with the rest of it.
        self.gui
            .draw_space(device, queue, &mut encoder, targets, eye, view_projection);

        // The scene is HDR and unclamped until here: the bloom, the grade and
        // the tone map all live in the resolve.
        self.post
            .resolve(&mut encoder, &target.create_view(&Default::default()));
        // After the resolve, not before it: a `ScreenGui` is an overlay, so
        // bloom, depth of field and the tone map must leave it alone. It takes
        // the texture rather than a view because it composites through a
        // non-sRGB one of its own (see `renderer::gui::pipeline::encoded`).
        self.gui.draw(
            device,
            queue,
            &mut encoder,
            target,
            size,
            &self.materials.bind_group,
        );

        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Which lamp casts this frame, and where its map looks.
    ///
    /// Roblox keeps the sun's direction pointing at the sun all night long and
    /// puts the moon opposite it (see [`crate::lighting`]), so the lamp that is
    /// actually above the horizon is the sun by day and the fill term after
    /// dusk — and it is that one, not always the sun, whose shadows are drawn.
    fn sun_shadow(&self, from: Viewpoint, aspect: f32) -> (Lamp, Fit) {
        if !self.lighting.global_shadows || !self.quality.shadows {
            return (Lamp::None, Fit::unfitted());
        }

        let (light, lamp) = if self.lighting.sun_direction.y >= 0.0 {
            (self.lighting.sun_direction, Lamp::Sun)
        } else {
            (-self.lighting.sun_direction, Lamp::Fill)
        };
        let frustum = self
            .camera
            .frustum_corners(from, aspect, self.quality.shadow_distance);

        (
            lamp,
            shadow::fit(light, &frustum, &self.bounds, self.quality.shadow_map_size),
        )
    }
}
