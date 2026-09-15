//! Draws a scene's parts, the decals projected onto them, its sky and its
//! celestial bodies into any render target, window or texture alike.

mod beam;
mod cull;
mod envmap;
mod filemesh;
mod geometry;
mod gui;
mod instance;
mod lighting;
mod material;
mod mesh;
mod particles;
mod pass;
mod pipeline;
mod post;
mod selection;
mod shadow;
mod shaped;
mod skybox;
mod stars;
mod sun;
mod switch;
mod texture;
mod textured;
mod trail;
mod translucent;

use rbx_dom::Ref;

use crate::camera::{Camera, Frustum, Viewpoint};
use crate::lighting::{Lighting, LocalLight};
use crate::quality::QualityProfile;
use crate::scene::{Bounds, Part, Scene};
use crate::textures::Decor;
use beam::Beams;
use cull::MainCull;
use envmap::EnvMap;
use geometry::Meshes;
use gui::Gui;
use lighting::LightingRaw;
use material::Materials;
use particles::Particles;
use pipeline::{Frame, Shared, Target};
use post::Post;
use selection::Selection;
use shadow::{Fit, Lamp, Shadows};
use shaped::Shaped;
use stars::Stars;
use trail::Trails;
use translucent::Translucent;

/// Only ever seen where a scene has no `Sky`, or where one of its six panels
/// would not resolve.
const CLEAR_COLOR: wgpu::Color = wgpu::Color::BLACK;

/// Everything a frame is drawn from. The three always travel together — both
/// render paths read the same DOM once and hand the lot straight on — so they
/// move as one argument rather than three.
#[derive(Clone, Copy)]
pub(crate) struct World<'a> {
    pub(crate) scene: &'a Scene,
    pub(crate) decor: &'a Decor,
    pub(crate) lighting: &'a Lighting,
    /// The place's `PointLight`s, `SpotLight`s and `SurfaceLight`s. Empty with
    /// `--no-lights`, and static: nothing in this viewer moves a light, so they
    /// are uploaded once and never rewritten.
    pub(crate) lights: &'a [LocalLight],
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
        // Read once here rather than kept as a whole `Scene`: nothing in a
        // built scene's placements changes afterwards, and `Renderer::new` has
        // no other reason to hold on to it.
        let selection = Selection::new(device, target, &layout, scene.placements());

        Renderer {
            opaque,
            blended,
            frame,
            lighting: *lighting,
            all_lights: lights.to_vec(),
            lights: allowed,
            env,
            meshes,
            materials: Materials::new(device, queue, &material_layout, scene.materials(), quality),
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
            beams: Beams::new(device, queue, target, scene.beams(), quality),
            trails: Trails::new(device, queue, target, scene.trails(), quality),
            particles: Particles::new(device, queue, target, scene.particle_emitters(), quality),
            selection,
            gui: Gui::new(
                device,
                queue,
                format,
                target,
                (scene.gui_screens(), scene.gui_spaces()),
                quality,
            ),
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

    /// Replaces the outlined selection, rebuilding its tiny vertex buffer right
    /// away rather than waiting for the next `draw`.
    pub(crate) fn set_selection(&mut self, device: &wgpu::Device, referents: &[Ref]) {
        self.selection.set(device, referents);
    }

    /// Applies a `Lighting`/`Atmosphere`/`Clouds`/`PostEffect`/`Light` edit
    /// without rebuilding the scene.
    ///
    /// The constant terms (`sun_direction`, `fog`, `clouds`, `Effects`, …) are
    /// already folded into the per-frame uniform (see [`Renderer::draw`]), so
    /// swapping `self.lighting`/`self.post`'s copy in is enough for those; only
    /// the local-light storage buffer is written here rather than every frame,
    /// since nothing else ever touches it after [`Renderer::new`].
    ///
    /// `false` when `lights.len()` differs from what was last uploaded — a
    /// property edit alone never adds or removes a `Light`, so this is a
    /// safety net rather than an expected path, and it means the buffer is the
    /// wrong size to write into; the caller falls back to a full reload.
    pub(crate) fn update_lighting(
        &mut self,
        queue: &wgpu::Queue,
        lighting: Lighting,
        lights: &[LocalLight],
    ) -> bool {
        if lights.len() != self.all_lights.len() {
            return false;
        }
        self.lighting = lighting;
        self.post.set_effects(lighting.effects);
        lighting::local_lights_write(queue, &self.lights_buffer, &lights[..self.lights]);
        self.all_lights = lights.to_vec();
        true
    }

    /// Patches one `BasePart`'s GPU instance — the opaque or translucent copy,
    /// and its shadow caster if it has one — for a Properties-panel edit that
    /// touched only that instance (see `crate::scene::Scene::patch_part`,
    /// which already refused anything that would cross a GPU bucket).
    ///
    /// `false` means `part.referent` was not found exactly where the scene's
    /// own bucket check said it would be — a defensive fallback that should
    /// never actually trigger, not a documented case a caller needs to reason
    /// about.
    pub(crate) fn patch_instance(&mut self, queue: &wgpu::Queue, part: &Part) -> bool {
        let drawn = if part.is_translucent() {
            self.translucent.patch(part)
        } else {
            self.shaped.patch(queue, part)
        };
        if !drawn {
            return false;
        }
        if part.casts_shadow()
            && !self
                .shadows
                .patch_caster(queue, part.referent, part.kind, part.transform)
        {
            return false;
        }
        true
    }

    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        size: (u32, u32),
        from: Viewpoint,
    ) {
        // A minimized window reports a zero-sized surface, which no texture can match.
        if size.0 == 0 || size.1 == 0 {
            return;
        }

        let aspect = size.0 as f32 / size.1 as f32;
        let eye = self.camera.eye_position(from);
        let view_projection = self.camera.view_projection(from, aspect);
        self.frame.write(queue, &view_projection);
        // The main pass's own visibility test: tight to the camera's frustum
        // and this level's render distance. The shadow pass below never uses
        // this — see `Fit::visible` — so a caster it culls can still land a
        // shadow inside the frame.
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
            sky.camera.write(queue, &rotation_only);
        }
        if let Some(stars) = &self.stars {
            stars.camera.write(queue, &rotation_only);
        }
        if let Some(bodies) = &self.bodies {
            bodies.camera.write(queue, &rotation_only);
        }
        // The same matrix the sun disc itself is drawn with, so the god-rays
        // in the resolve can never point anywhere the disc is not.
        let sun_screen = sun::sun_screen_position(self.lighting.sun_direction, &rotation_only);

        // Both re-sort and re-upload their instances; neither may run inside the
        // render pass that reads those buffers.
        self.translucent.prepare(queue, eye, &cull);
        self.filemesh.prepare(queue, eye);

        if self.post.prepare(device, queue, size, sun_screen).is_none() {
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
        self.beams
            .draw(queue, device, &mut encoder, targets, eye, view_projection);
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
        self.post.resolve(&mut encoder, target);
        // After the resolve, not before it: a `ScreenGui` is an overlay, so
        // bloom, depth of field and the tone map must leave it alone.
        self.gui.draw(device, queue, &mut encoder, target, size);

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
