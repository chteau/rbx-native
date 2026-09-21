//! The sun's shadow map: one orthographic depth pass over every caster in the
//! scene, sampled back by `lighting.wgsl` through a comparison sampler.
//!
//! The casters are the pass's own (see [`casters`]) rather than borrowed from the
//! colour passes: a depth pass needs positions and a model matrix and nothing
//! else, so the unit shapes are re-instanced against the meshes
//! [`super::geometry`] already holds, and a file mesh gets a position-only copy
//! of its vertices — smaller than the textured copy it is drawn from, and it
//! spares every colour-pass batch a caster-only instance buffer of its own.
//!
//! Acne is kept off with a slope-scaled depth bias rather than by culling front
//! faces: downloaded meshes do not all agree on their winding (see
//! `renderer::filemesh`), so "front" is not a face this renderer can name.
//! The receiving side adds a normal offset — see `lamp_visibility` in
//! `lighting.wgsl` — which is what keeps the PCF kernel from shadowing the very
//! surface it is filtering.

pub(in crate::renderer) mod casters;
mod fit;
pub(super) mod local;
pub(super) mod point;

use super::cull;
use super::geometry::Meshes;
use super::mesh::Vertex;
use super::slots::keyed::Keyed;
use crate::quality::QualityProfile;
use crate::scene::Scene;
use casters::{CasterRaw, MeshBatches, ShapeBatches, CASTER_ATTRIBUTES, POSITION_ATTRIBUTE};

pub(super) use fit::{fit, Fit};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Side of one `SpotLight`/`SurfaceLight`'s own map. Fixed rather than a
/// quality knob: the cap on how many lights get one already trades quality for
/// cost (see `QualityProfile::local_shadow_lights_max`), and a lantern close up
/// is a small enough thing on screen that one map size covers every level.
const LOCAL_SIZE: u32 = 1024;

/// Side of one face of a `PointLight`'s cube. Half the cone lights' own map:
/// a point light spends six of these where a cone light spends one, and
/// halving the side is what keeps a cube at the same memory a single cone
/// map costs plus half again, rather than at six times it.
const POINT_SIZE: u32 = 512;

/// How many `PointLight`s may cast at once, from the same level knob the
/// cone lights' cap comes from. A quarter of it, rounded down: six faces
/// each, and a level that allows no cone shadows at all allows no point
/// ones either.
fn point_cap(quality: &QualityProfile) -> usize {
    quality.local_shadow_lights_max / 4
}

/// Exactly one texel's worth of slope, which is how much depth a caster tilted
/// away from the lamp can gain across the texel it is quantized into; the
/// constant term only covers the last float rounding. Both stay deliberately
/// small, the receiver's normal offset picking up what the PCF kernel adds on
/// top: this bias is measured in depth-buffer units, so on a place whose bounds
/// span thousands of studs one extra unit of `slope_scale` is worth a foot of
/// world — enough to eat the whole shadow of a one-stud-thick spawn pad.
const DEPTH_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState {
    constant: 2,
    slope_scale: 1.0,
    clamp: 0.0,
};

/// Standard depth, not the reversed-Z the colour passes use: an orthographic
/// projection is linear in depth, so reversing it buys none of the precision it
/// buys a perspective one, and a plain `Less` keeps the comparison sampler's
/// `LessEqual` reading the right way round.
const CLEAR_DEPTH: f32 = 1.0;

const MATRIX_SIZE: wgpu::BufferAddress = std::mem::size_of::<[[f32; 4]; 4]>() as _;

/// The depth map, its pipelines and the scene's casters.
///
/// Built once: the map's size comes from the quality level rather than from the
/// window, so a resize never touches it and the bind group the colour passes
/// sample it through stays valid for the whole run.
pub(super) struct Shadows {
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    light: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    shapes: wgpu::RenderPipeline,
    meshes: wgpu::RenderPipeline,
    /// The unit-shape casters, then the file-mesh ones — each keyed the way
    /// its colour pass is, with their own referent index so
    /// [`Shadows::sync_caster`]/[`Shadows::sync_mesh_caster`] can move one
    /// between batches.
    shape_batches: ShapeBatches,
    mesh_batches: MeshBatches,
    /// Kept only so [`Shadows::set_quality`] can rebuild [`Shadows::local_bind_groups`]
    /// around a fresh set of per-light buffers: the layout itself never changes.
    light_layout: wgpu::BindGroupLayout,
    /// The whole array, for the comparison sampling every surface pass does
    /// (see `lights.wgsl`'s `local_shadow_map`).
    local_view: wgpu::TextureView,
    /// One single-layer view per array slot, for `render_local` to draw into.
    /// Empty layers past what [`local::select`] filled this frame are simply
    /// never read: [`local::pack`] marks them unshadowed on the CPU side.
    local_layers: Vec<wgpu::TextureView>,
    /// One small uniform per slot rather than one reused buffer: every slot is
    /// written and drawn from in the same command encoder before it is
    /// submitted, and a buffer wgpu has not yet flushed to the GPU may not be
    /// written a second time and still have both passes see their own value.
    local_buffers: Vec<wgpu::Buffer>,
    local_bind_groups: Vec<wgpu::BindGroup>,
    /// The `PointLight` cubes: the same three arrays as the cone lights
    /// above, six layers to a light — see `renderer::shadow::point`.
    point_view: wgpu::TextureView,
    point_layers: Vec<wgpu::TextureView>,
    point_buffers: Vec<wgpu::Buffer>,
    point_bind_groups: Vec<wgpu::BindGroup>,
    /// Every selected cube's six matrices, as the shader reads them back —
    /// one entry per layer of [`Shadows::point_view`], rewritten whenever
    /// the selection changes.
    point_faces: wgpu::Buffer,
    /// Which lights the cubes currently hold, so a frame that would redraw
    /// the very same six faces from the very same place does not.
    ///
    /// The cone maps redraw every frame because one pass is cheap; six per
    /// light is not, and nothing in this viewer moves a light or a caster
    /// without saying so — a caster edit sets this back to `None`.
    point_state: Option<Vec<usize>>,
}

impl Shadows {
    pub(super) fn new(device: &wgpu::Device, scene: &Scene, quality: &QualityProfile) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rbxview shadow light"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let light = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview shadow light"),
            size: MATRIX_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cubes = point_cap(quality);
        let (point_view, point_layers) = point_map(device, cubes);
        let point_buffers = local_buffers(device, cubes * point::FACES);
        let point_bind_groups = local_bind_groups(device, &layout, &point_buffers);
        let (local_view, local_layers) = local_map(device, quality.local_shadow_lights_max);
        let local_buffers = local_buffers(device, quality.local_shadow_lights_max);
        let local_bind_groups = local_bind_groups(device, &layout, &local_buffers);
        let shape_batches = casters::shape_batches(device, scene);
        let mesh_batches =
            casters::mesh_batches(device, scene.resolved_file_meshes(), &mut Vec::new());

        Shadows {
            view: map(device, quality.shadow_map_size),
            // Linear filtering on a comparison sampler is the hardware's own
            // 2x2 PCF: every tap of the kernel already comes back partly lit.
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("rbxview shadow"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                compare: Some(wgpu::CompareFunction::LessEqual),
                ..Default::default()
            }),
            bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rbxview shadow light"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: light.as_entire_binding(),
                }],
            }),
            shapes: pipeline(device, &layout, std::mem::size_of::<Vertex>() as _),
            meshes: pipeline(device, &layout, std::mem::size_of::<[f32; 3]>() as _),
            shape_batches,
            mesh_batches,
            light,
            local_view,
            local_layers,
            local_buffers,
            local_bind_groups,
            point_view,
            point_layers,
            point_faces: point_faces_buffer(device, cubes),
            point_buffers,
            point_bind_groups,
            point_state: None,
            light_layout: layout,
        }
    }

    /// Reallocates the sun's map and the local one at the level's own size.
    /// Only textures and buffers: the casters, the pipelines and the layouts
    /// are all size-agnostic, and the PCF radius and the reach live in the
    /// lighting uniform instead.
    ///
    /// The caller must rebind every bind group holding the old views or the old
    /// [`Shadows::local_view`]/light buffer.
    pub(super) fn set_quality(&mut self, device: &wgpu::Device, quality: &QualityProfile) {
        self.view = map(device, quality.shadow_map_size);
        let (view, layers) = local_map(device, quality.local_shadow_lights_max);
        self.local_view = view;
        self.local_layers = layers;
        self.local_buffers = local_buffers(device, quality.local_shadow_lights_max);
        self.local_bind_groups = local_bind_groups(device, &self.light_layout, &self.local_buffers);

        let cubes = point_cap(quality);
        let (point_view, point_layers) = point_map(device, cubes);
        self.point_view = point_view;
        self.point_layers = point_layers;
        self.point_buffers = local_buffers(device, cubes * point::FACES);
        self.point_bind_groups = local_bind_groups(device, &self.light_layout, &self.point_buffers);
        self.point_faces = point_faces_buffer(device, cubes);
        self.point_state = None;
    }

    /// Replaces every caster with `scene`'s, keeping the maps, the pipelines
    /// and the per-light buffers — none of which a scene decides — and the
    /// position-only copy of every file mesh the new scene still casts with
    /// (see [`casters::mesh_batches`]).
    pub(super) fn rebuild(&mut self, device: &wgpu::Device, scene: &Scene) {
        self.point_state = None;
        self.shape_batches = casters::shape_batches(device, scene);
        let mut spare: Vec<(rbx_assets::AssetRef, casters::MeshGeometry)> =
            std::mem::replace(&mut self.mesh_batches, Keyed::new("rbxview shadow casters"))
                .into_groups()
                .into_iter()
                .map(|group| (group.key, group.extra))
                .collect();
        self.mesh_batches = casters::mesh_batches(device, scene.resolved_file_meshes(), &mut spare);
    }

    /// The shadow-map half of a single-instance edit (see
    /// `Renderer::sync_part`): rewrites the part's caster in place, moves
    /// it to its new shape's batch, drops it if it stopped casting, or adds
    /// it if it just started.
    pub(super) fn sync_caster(&mut self, device: &wgpu::Device, part: &crate::scene::Part) {
        self.point_state = None;
        casters::sync_shape(device, &mut self.shape_batches, part);
    }

    /// [`Shadows::sync_caster`] for a part drawn as a resolved file mesh —
    /// the shadow-map half of `Renderer::sync_part`'s mesh case. `false` only
    /// when a new batch would need a mesh `resolved` never downloaded.
    pub(super) fn sync_mesh_caster(
        &mut self,
        device: &wgpu::Device,
        resolved: &crate::scene::Resolved,
        instance: &crate::scene::ResolvedInstance,
    ) -> bool {
        self.point_state = None;
        casters::sync_mesh(device, &mut self.mesh_batches, resolved, instance)
    }

    /// Drops a file-mesh caster whose instance the scene no longer draws at
    /// all (see `Renderer::sync_part`); a no-op if it never cast.
    pub(super) fn remove_mesh_caster(&mut self, referent: rbx_dom::Ref) {
        self.point_state = None;
        self.mesh_batches.remove(referent);
    }

    /// Drops a box caster whose box no longer draws; a no-op if it never
    /// cast.
    pub(super) fn remove_caster(&mut self, id: crate::scene::PartId) {
        self.point_state = None;
        self.shape_batches.remove(id);
    }

    /// Uploads what the edits since the last frame owe both caster buffers
    /// — see `slots::Slots::flush`.
    pub(super) fn flush(&mut self, queue: &wgpu::Queue) {
        self.shape_batches.flush(queue);
        self.mesh_batches.flush(queue);
    }

    pub(super) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub(super) fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// The whole local shadow array, for `lights.wgsl`'s comparison sampling —
    /// read through the very same [`Shadows::sampler`] the sun map is.
    pub(super) fn local_view(&self) -> &wgpu::TextureView {
        &self.local_view
    }

    /// How many `SpotLight`/`SurfaceLight`s can cast a shadow this frame — the
    /// array's own layer count, and so the cap `local::select` must be given.
    pub(super) fn local_cap(&self) -> usize {
        self.local_layers.len()
    }

    /// The whole `PointLight` cube array, for `lights.wgsl` to sample —
    /// through the same comparison sampler everything else here is.
    pub(super) fn point_view(&self) -> &wgpu::TextureView {
        &self.point_view
    }

    /// The six matrices per cube the shader projects through, one entry per
    /// layer of [`Shadows::point_view`].
    pub(super) fn point_faces(&self) -> &wgpu::Buffer {
        &self.point_faces
    }

    /// How many `PointLight`s can cast at once — the cap
    /// `renderer::shadow::point::select` must be given.
    pub(super) fn point_cap(&self) -> usize {
        self.point_layers.len() / point::FACES
    }

    /// Redraws every selected `PointLight`'s six faces, and uploads the
    /// matrices the shader reads them back with.
    ///
    /// Skipped entirely when the same lights already hold the same cubes
    /// and nothing they could cast has changed: six passes a frame per
    /// light is the one place in this file where redrawing regardless
    /// would show up in a frame time.
    pub(super) fn render_points(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        meshes: &Meshes,
        selected: &[point::Selected],
    ) {
        debug_assert!(selected.len() <= self.point_cap());
        let wanted: Vec<usize> = selected.iter().map(|light| light.index).collect();
        if self.point_state.as_ref() == Some(&wanted) {
            return;
        }

        let mut matrices = vec![[[0.0f32; 4]; 4]; self.point_layers.len().max(point::FACES)];
        for (cube, light) in selected.iter().enumerate() {
            for (face, matrix) in light.faces.iter().enumerate() {
                let layer = cube * point::FACES + face;
                matrices[layer] = matrix.to_cols_array_2d();
                queue.write_buffer(
                    &self.point_buffers[layer],
                    0,
                    bytemuck::cast_slice(&matrix.to_cols_array()),
                );
                let mut pass = self.begin(encoder, &self.point_layers[layer]);
                self.draw_casters(&mut pass, meshes, &self.point_bind_groups[layer], None);
            }
        }
        queue.write_buffer(&self.point_faces, 0, bytemuck::cast_slice(&matrices));
        self.point_state = Some(wanted);
    }

    /// Redraws the whole map for this frame's fit. Cheap enough to do every
    /// frame — the fit is snapped to texels, so a still camera redraws the same
    /// depths rather than subtly different ones.
    pub(super) fn render(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        meshes: &Meshes,
        fit: &Fit,
    ) {
        queue.write_buffer(
            &self.light,
            0,
            bytemuck::cast_slice(&fit.view_projection.to_cols_array()),
        );
        let mut pass = self.begin(encoder, &self.view);
        self.draw_casters(&mut pass, meshes, &self.bind_group, Some(fit));
    }

    /// Redraws every selected local light's own map into its assigned array
    /// layer, sharing the very same depth-only pipelines and casters the sun
    /// pass draws with (see [`Shadows::draw_casters`]).
    ///
    /// `selected` must not hold more entries than [`Shadows::local_cap`] — the
    /// caller gets that cap from here in the first place (see
    /// `Renderer::draw`), so a mismatch would be this module's own bug rather
    /// than a place file's.
    pub(super) fn render_local(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        meshes: &Meshes,
        selected: &[local::Selected],
    ) {
        debug_assert!(selected.len() <= self.local_cap());
        for (layer, light) in selected.iter().enumerate() {
            queue.write_buffer(
                &self.local_buffers[layer],
                0,
                bytemuck::cast_slice(&light.view_projection.to_cols_array()),
            );
            let mut pass = self.begin(encoder, &self.local_layers[layer]);
            self.draw_casters(&mut pass, meshes, &self.local_bind_groups[layer], None);
        }
    }

    /// One depth-only pass over `view`, cleared and ready for casters.
    fn begin<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> wgpu::RenderPass<'e> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview shadow map"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR_DEPTH),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// Every caster in the scene, from whichever light `bind_group`'s own
    /// buffer currently holds the view-projection of — the sun's or one local
    /// light's, the pass and the geometry being identical either way.
    ///
    /// `cull` is `Some` only for the sun pass, whose own map can cover a huge
    /// area: it skips shape casters [`Fit::visible`] says cannot land in it,
    /// same as `shaped::Shaped::draw` does for the main pass but against a
    /// deliberately wider test. A local light's own map is small and close by
    /// construction, so its casters are drawn unfiltered (`None`) — see
    /// `Renderer::draw`'s call to [`Shadows::render_local`].
    fn draw_casters<'p>(
        &'p self,
        pass: &mut wgpu::RenderPass<'p>,
        meshes: &Meshes,
        bind_group: &'p wgpu::BindGroup,
        cull: Option<&Fit>,
    ) {
        pass.set_pipeline(&self.shapes);
        pass.set_bind_group(0, bind_group, &[]);
        // A batch an edit emptied keeps its buffer but has nothing to draw.
        for batch in self
            .shape_batches
            .groups()
            .iter()
            .filter(|batch| batch.slots.count() > 0)
        {
            let Some(mesh) = meshes.get(batch.key) else {
                continue;
            };
            match cull {
                Some(fit) => {
                    let runs = cull::visible_runs(batch.slots.count(), |index| {
                        let (center, radius) = batch.slots.side(index);
                        fit.visible(center, radius)
                    });
                    if runs.is_empty() {
                        continue;
                    }
                    pass.set_vertex_buffer(1, batch.slots.buffer().slice(..));
                    for run in runs {
                        mesh.draw_range(pass, run);
                    }
                }
                None => {
                    pass.set_vertex_buffer(1, batch.slots.buffer().slice(..));
                    mesh.draw(pass, batch.slots.count());
                }
            }
        }

        pass.set_pipeline(&self.meshes);
        pass.set_bind_group(0, bind_group, &[]);
        for batch in self
            .mesh_batches
            .groups()
            .iter()
            .filter(|batch| batch.slots.count() > 0)
        {
            let geometry = &batch.extra;
            pass.set_vertex_buffer(0, geometry.vertices.slice(..));
            pass.set_vertex_buffer(1, batch.slots.buffer().slice(..));
            pass.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..geometry.index_count, 0, 0..batch.slots.count());
        }
    }
}

/// The depth map itself.
///
/// Allocated even where the level casts no shadows: the bind group is shared by
/// every surface pipeline, and a missing texture would mean a second layout
/// rather than one unread kilobyte of depth.
fn map(device: &wgpu::Device, side: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rbxview shadow map"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// The local shadow texture array — one whole-array view for sampling, and one
/// single-layer view per slot for `render_local` to draw into.
///
/// Allocated at `cap.max(1)` layers even when `cap` is 0, for the reason
/// [`map`] gives for the sun's own map: bind group 0 is shared by every
/// surface pipeline whatever the level allows.
fn local_map(device: &wgpu::Device, cap: usize) -> (wgpu::TextureView, Vec<wgpu::TextureView>) {
    let layers = cap.max(1) as u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rbxview local shadow map"),
        size: wgpu::Extent3d {
            width: LOCAL_SIZE,
            height: LOCAL_SIZE,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    let array_view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("rbxview local shadow map (array)"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layer_views = (0..layers)
        .map(|layer| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("rbxview local shadow map (layer)"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();

    (array_view, layer_views)
}

/// The `PointLight` cube array: six layers per cube, each drawn into on its
/// own and all sampled through one array view — see
/// `renderer::shadow::point` for why this is an array of perspectives
/// rather than a cube map.
fn point_map(device: &wgpu::Device, cubes: usize) -> (wgpu::TextureView, Vec<wgpu::TextureView>) {
    let layers = (cubes * point::FACES).max(1) as u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rbxview point shadow map"),
        size: wgpu::Extent3d {
            width: POINT_SIZE,
            height: POINT_SIZE,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    let array_view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("rbxview point shadow map (array)"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layer_views = (0..layers)
        .map(|layer| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("rbxview point shadow map (face)"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();

    (array_view, layer_views)
}

/// The matrices `lights.wgsl` projects a fragment through, one per layer of
/// [`point_map`]'s array. Never empty, for the same reason the light buffer
/// itself never is: a storage binding has to point at something.
fn point_faces_buffer(device: &wgpu::Device, cubes: usize) -> wgpu::Buffer {
    let layers = (cubes * point::FACES).max(point::FACES);
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rbxview point shadow faces"),
        size: MATRIX_SIZE * layers as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// One small view-projection uniform per array slot: every slot is written and
/// drawn from within the same command encoder before it is submitted, and a
/// single reused buffer would let a later write race the earlier pass that
/// reads it (see [`Shadows::render_local`]).
fn local_buffers(device: &wgpu::Device, cap: usize) -> Vec<wgpu::Buffer> {
    (0..cap.max(1))
        .map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview local shadow light"),
                size: MATRIX_SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
        .collect()
}

fn local_bind_groups(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffers: &[wgpu::Buffer],
) -> Vec<wgpu::BindGroup> {
    buffers
        .iter()
        .map(|buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rbxview local shadow light"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        })
        .collect()
}

/// One depth-only pipeline. `stride` is the only thing the two differ by: both
/// read a `vec3` position out of slot 0, out of buffers packed for different
/// colour passes.
fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    stride: wgpu::BufferAddress,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rbxview shadow"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shadow.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview shadow"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview shadow"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: stride,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &POSITION_ATTRIBUTE,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CasterRaw>() as _,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &CASTER_ATTRIBUTES,
                }),
            ],
        },
        primitive: wgpu::PrimitiveState {
            // See the module doc: winding is not dependable here, so neither
            // face may be dropped.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: DEPTH_BIAS,
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: None,
        multiview_mask: None,
        cache: None,
    })
}

/// Which lamp `lighting.wgsl` has to darken, and whether there is one at all.
///
/// Roblox keeps one directional lamp above the horizon at a time: the sun by
/// day, the moon — which this renderer draws as the fill lamp at `-L`, see
/// `crate::lighting` — once it has set. Whichever is up is the one that casts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Lamp {
    None,
    Sun,
    Fill,
}

impl Lamp {
    /// `0` off, `+1` the sun term, `-1` the fill term: what the uniform carries
    /// and what `shade` selects on.
    pub(super) fn marker(self) -> f32 {
        match self {
            Lamp::None => 0.0,
            Lamp::Sun => 1.0,
            Lamp::Fill => -1.0,
        }
    }
}

#[cfg(test)]
#[path = "shadow/tests.rs"]
mod tests;
