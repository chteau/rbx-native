//! Real geometry for `MeshPart` and file-backed `SpecialMesh` instances: one
//! vertex/index buffer per unique mesh asset (LOD 0 only), and one instance
//! buffer per (mesh, skin) pair drawn against it.
//!
//! Untextured meshes reuse the shape pass's own shader (`shader.wgsl`) and
//! vertex/instance layout, just built as a separate pipeline rather than
//! literally the same `wgpu::RenderPipeline` — downloaded community meshes
//! don't all agree on winding, so these pipelines disable back-face culling
//! rather than risk drawing them inside-out. A mesh's own `TextureID` gets a UV
//! pipeline, mirroring `textured.rs` but instanced instead of pre-baked into
//! world space, and a `SurfaceAppearance` a third one that adds a tangent
//! attribute and its own PBR map set (see [`appearance`]).
//!
//! A `MeshPart` carrying a `Transparency` goes to the blended pipelines instead,
//! which need a camera-dependent draw order — see [`FileMeshes::prepare`].

mod appearance;
mod images;
mod patch;
mod pipelines;
mod vertex;

use std::collections::HashMap;

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use super::instance::InstanceRaw;
use super::mesh::Vertex as BoxVertex;
use super::pipeline::{Bindings, Target};
use super::texture;
use crate::quality::QualityProfile;
use crate::scene::{Resolved, ResolvedInstance};
use images::{Binding, Images};
use pipelines::{Layouts, Pipelines, Skin};
use vertex::{AppearanceVertex, TexturedVertex};

/// One draw call's worth of geometry: a mesh, the instances of it, and what
/// skins them.
struct Batch {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    instances: wgpu::Buffer,
    instance_count: u32,
    skin: Skin,
}

/// A translucent batch keeps its instances on the CPU as well: blending is
/// order-dependent, so both the instances within a batch and the batches
/// themselves are re-sorted every frame.
struct Blended {
    batch: Batch,
    /// Each instance's referent, world-space centre and GPU record, in the
    /// order [`FileMeshes::prepare`] last sorted them — the referent is what
    /// [`FileMeshes::patch`] finds an item by, since that order changes.
    items: Vec<(Ref, Vec3, InstanceRaw)>,
    /// This frame's distance to the furthest instance in the batch.
    depth: f32,
}

/// GPU state for every real mesh in a scene: two pipeline sets (opaque and
/// blended, each with its three skins) and the batches drawn through them.
pub(super) struct FileMeshes {
    opaque_pipelines: Pipelines,
    blended_pipelines: Pipelines,
    image_layout: wgpu::BindGroupLayout,
    appearance_layout: wgpu::BindGroupLayout,
    images: Images,
    appearances: appearance::Sets,
    opaque: Vec<Batch>,
    blended: Vec<Blended>,
    /// Where each opaque instance sits — its batch in `opaque` and its offset
    /// within it — so a single-instance edit (see [`FileMeshes::patch`]) can
    /// write straight into the buffer instead of rebuilding it, and which
    /// `blended` batch holds each translucent one.
    opaque_index: HashMap<Ref, (usize, u32)>,
    blended_index: HashMap<Ref, usize>,
    order: Vec<usize>,
}

impl FileMeshes {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
        material_layout: &wgpu::BindGroupLayout,
        resolved: &Resolved,
        quality: &QualityProfile,
    ) -> Self {
        let mut opaque = Vec::new();
        let mut blended = Vec::new();
        let mut opaque_index = HashMap::new();
        let mut blended_index = HashMap::new();
        let image_layout = texture::layout(device);
        let appearance_layout = appearance::layout(device);
        // Clamped, not repeated: a mesh's UVs are an authored atlas, not a
        // tiling pattern the way a `Texture` face is.
        let sampler = texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy);
        let mut images = Images::default();
        let binding = Binding {
            layout: &image_layout,
            sampler: &sampler,
            max_size: quality.texture_max_size,
        };

        for (key, group) in group_by_mesh_and_skin(resolved) {
            let Some(mesh) = resolved.meshes.get(&key.mesh) else {
                continue;
            };
            let Some(skin) = images.slot(device, queue, binding, resolved, &key) else {
                continue;
            };

            // Scanning the colour map for alpha is a whole-image pass, so it
            // happens once per batch rather than once per instance.
            let blends = key
                .appearance
                .and_then(|index| resolved.appearances.get(index))
                .is_some_and(|set| set.is_translucent(&resolved.images));
            let (see_through, still): (Vec<_>, Vec<_>) = group
                .into_iter()
                .partition(|instance| blends || instance.alpha < 1.0);
            if !still.is_empty() {
                for (offset, instance) in still.iter().enumerate() {
                    opaque_index.insert(instance.referent, (opaque.len(), offset as u32));
                }
                opaque.push(build(device, mesh, skin, &still));
            }
            if !see_through.is_empty() {
                for instance in &see_through {
                    blended_index.insert(instance.referent, blended.len());
                }
                blended.push(Blended {
                    batch: build(device, mesh, skin, &see_through),
                    items: see_through
                        .iter()
                        .map(|i| (i.referent, center(i), raw(i)))
                        .collect(),
                    depth: 0.0,
                });
            }
        }

        let layouts = Layouts {
            frame: frame_layout,
            image: &image_layout,
            materials: material_layout,
            appearance: &appearance_layout,
        };
        FileMeshes {
            opaque_pipelines: pipelines::build(device, target, &layouts, false),
            blended_pipelines: pipelines::build(device, target, &layouts, true),
            appearances: appearance::Sets::new(
                device,
                queue,
                Binding {
                    layout: &appearance_layout,
                    sampler: &sampler,
                    max_size: quality.texture_max_size,
                },
                resolved,
            ),
            image_layout,
            appearance_layout,
            images,
            opaque,
            blended,
            opaque_index,
            blended_index,
            order: Vec::new(),
        }
    }

    /// Re-views every mesh texture and `SurfaceAppearance` map at the new cap and
    /// anisotropy. Nothing is decoded, uploaded or re-batched.
    pub(super) fn set_quality(&mut self, device: &wgpu::Device, quality: &QualityProfile) {
        let sampler = texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy);
        self.images.rebind(
            device,
            Binding {
                layout: &self.image_layout,
                sampler: &sampler,
                max_size: quality.texture_max_size,
            },
        );
        self.appearances.rebind(
            device,
            Binding {
                layout: &self.appearance_layout,
                sampler: &sampler,
                max_size: quality.texture_max_size,
            },
        );
    }

    /// Rebuilds both pipeline sets for a new sample count. `frame` and `materials`
    /// are the renderer's own layouts, the other two being this module's.
    pub(super) fn set_target(
        &mut self,
        device: &wgpu::Device,
        target: Target,
        (frame, materials): (&wgpu::BindGroupLayout, &wgpu::BindGroupLayout),
    ) {
        let layouts = Layouts {
            frame,
            image: &self.image_layout,
            materials,
            appearance: &self.appearance_layout,
        };
        self.opaque_pipelines = pipelines::build(device, target, &layouts, false);
        self.blended_pipelines = pipelines::build(device, target, &layouts, true);
    }

    /// Re-sorts what has to blend for this frame's camera, furthest first: the
    /// instances inside each batch, then the batches against each other.
    ///
    /// Ordering whole batches by their furthest instance is coarse — two
    /// interleaved translucent meshes can still blend in the wrong order — but
    /// each batch is one mesh with one vertex buffer, and splitting those apart
    /// would cost a draw call per instance.
    pub(super) fn prepare(&mut self, queue: &wgpu::Queue, eye: Vec3) {
        for blended in &mut self.blended {
            blended
                .items
                .sort_by(|left, right| distance(right.1, eye).total_cmp(&distance(left.1, eye)));
            blended.depth = blended
                .items
                .first()
                .map_or(0.0, |(_, center, _)| distance(*center, eye));

            let instances: Vec<InstanceRaw> =
                blended.items.iter().map(|(_, _, raw)| *raw).collect();
            queue.write_buffer(
                &blended.batch.instances,
                0,
                bytemuck::cast_slice(&instances),
            );
        }

        self.order.clear();
        self.order.extend(0..self.blended.len());
        let depth = |index: usize| self.blended[index].depth;
        self.order
            .sort_by(|&left, &right| depth(right).total_cmp(&depth(left)));
    }

    /// Draws the opaque meshes. The caller must have the frame bind group ready
    /// at group 0; this switches pipelines itself (unlike `Shaped`, which
    /// piggybacks on the shape pass's already-bound one).
    pub(super) fn draw_opaque(&self, pass: &mut wgpu::RenderPass<'_>, bindings: Bindings<'_>) {
        self.run(pass, bindings, &self.opaque_pipelines, self.opaque.iter());
    }

    /// Draws the translucent meshes in the order [`FileMeshes::prepare`] worked
    /// out, after everything opaque in the frame.
    pub(super) fn draw_blended(&self, pass: &mut wgpu::RenderPass<'_>, bindings: Bindings<'_>) {
        let batches = self.order.iter().map(|&index| &self.blended[index].batch);
        self.run(pass, bindings, &self.blended_pipelines, batches);
    }

    fn run<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        bindings: Bindings<'_>,
        pipelines: &Pipelines,
        batches: impl Iterator<Item = &'a Batch>,
    ) {
        let mut bound: Option<&wgpu::RenderPipeline> = None;
        for batch in batches {
            let pipeline = pipelines.get(batch.skin);
            if !bound.is_some_and(|previous| std::ptr::eq(previous, pipeline)) {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bindings.frame, &[]);
                pass.set_bind_group(1, bindings.materials, &[]);
                bound = Some(pipeline);
            }
            match batch.skin {
                Skin::Plain => {}
                Skin::Image(slot) => {
                    pass.set_bind_group(2, &self.images.bind_groups[slot], &[]);
                }
                Skin::Appearance(slot) => {
                    pass.set_bind_group(2, &self.appearances.bind_groups[slot], &[]);
                }
            }

            pass.set_vertex_buffer(0, batch.vertices.slice(..));
            pass.set_vertex_buffer(1, batch.instances.slice(..));
            // File mesh vertex counts aren't bounded the way the procedural
            // shapes are, so indices stay 32-bit rather than risking silent
            // truncation.
            pass.set_index_buffer(batch.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..batch.index_count, 0, 0..batch.instance_count);
        }
    }
}

fn distance(center: Vec3, eye: Vec3) -> f32 {
    (center - eye).length_squared()
}

fn center(instance: &ResolvedInstance) -> Vec3 {
    instance.model.col(3).truncate()
}

fn raw(instance: &ResolvedInstance) -> InstanceRaw {
    InstanceRaw::new(
        instance.model.to_cols_array_2d(),
        instance.color,
        instance.alpha,
        instance.reflectance,
        instance.material,
    )
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct GroupKey {
    mesh: AssetRef,
    pub(super) texture: Option<AssetRef>,
    pub(super) appearance: Option<usize>,
}

/// Groups resolved instances by (mesh, skin), preserving first-seen order so
/// batch construction is deterministic run to run.
fn group_by_mesh_and_skin(resolved: &Resolved) -> Vec<(GroupKey, Vec<&ResolvedInstance>)> {
    let mut order: Vec<GroupKey> = Vec::new();
    let mut groups: HashMap<GroupKey, Vec<&ResolvedInstance>> = HashMap::new();

    for instance in &resolved.instances {
        let key = GroupKey {
            mesh: instance.mesh.clone(),
            texture: instance.texture.clone(),
            appearance: instance.appearance,
        };
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(instance);
    }

    order
        .into_iter()
        .map(|key| {
            let instances = groups.remove(&key).unwrap_or_default();
            (key, instances)
        })
        .collect()
}

/// Each skin reads a different vertex format, which is the only thing that
/// differs between the three.
///
/// A mesh drawn under two skins is converted twice; that is a handful of meshes
/// per scene, against a per-batch cache that would have to outlive the loop.
fn build(
    device: &wgpu::Device,
    mesh: &rbx_mesh::Mesh,
    skin: Skin,
    group: &[&ResolvedInstance],
) -> Batch {
    let vertices: Vec<u8> = match skin {
        Skin::Plain => bytemuck::cast_slice(
            &mesh
                .vertices
                .iter()
                .map(|vertex| BoxVertex::new(vertex.position, vertex.normal))
                .collect::<Vec<_>>(),
        )
        .to_vec(),
        Skin::Image(_) => bytemuck::cast_slice(&TexturedVertex::build(mesh)).to_vec(),
        Skin::Appearance(_) => bytemuck::cast_slice(&AppearanceVertex::build(mesh)).to_vec(),
    };
    let indices = mesh.lod0();
    let instances: Vec<InstanceRaw> = group.iter().copied().map(raw).collect();
    // Written afterwards by `FileMeshes::prepare` (a blended batch, every
    // frame) and `FileMeshes::patch` (either kind, one instance at a time).
    let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;

    Batch {
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview filemesh vertices"),
            contents: &vertices,
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview filemesh indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
        index_count: indices.len() as u32,
        instances: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview filemesh instances"),
            contents: bytemuck::cast_slice(&instances),
            usage,
        }),
        instance_count: instances.len() as u32,
        skin,
    }
}
