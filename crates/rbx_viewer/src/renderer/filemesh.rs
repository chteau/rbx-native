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
mod blended;
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
use super::rebuild::take_spare;
use super::slots::keyed::Keyed;
use super::slots::Roster;
use super::texture;
use crate::quality::QualityProfile;
use crate::scene::{Resolved, ResolvedInstance};
use blended::{blends, Blended};
use images::{Binding, Images};
use pipelines::{Layouts, Pipelines, Skin};
use vertex::{AppearanceVertex, TexturedVertex};

/// One batch's mesh, converted for the vertex format its skin reads, and
/// the skin itself — the payload of one opaque group, or one [`Blended`].
struct Geometry {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    skin: Skin,
}

/// One draw call: a batch's geometry and however many of its instances are
/// live.
struct Draw<'a> {
    geometry: &'a Geometry,
    instances: &'a wgpu::Buffer,
    count: u32,
}

/// The opaque batches, one per (mesh, skin), with the referent index a
/// single-instance edit moves an instance between them by.
type Opaque = Keyed<GroupKey, Geometry, InstanceRaw, ()>;

/// GPU state for every real mesh in a scene: two pipeline sets (opaque and
/// blended, each with its three skins) and the batches drawn through them.
pub(super) struct FileMeshes {
    opaque_pipelines: Pipelines,
    blended_pipelines: Pipelines,
    image_layout: wgpu::BindGroupLayout,
    appearance_layout: wgpu::BindGroupLayout,
    /// What every batch's texture was bound with, kept so a batch an edit
    /// creates later (see [`FileMeshes::sync`]) binds its own the same way.
    sampler: wgpu::Sampler,
    texture_max_size: u32,
    images: Images,
    appearances: appearance::Sets,
    opaque: Opaque,
    blended: Vec<Blended>,
    /// Which `blended` batch holds each translucent instance.
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
        let image_layout = texture::layout(device);
        let appearance_layout = appearance::layout(device);
        // Clamped, not repeated: a mesh's UVs are an authored atlas, not a
        // tiling pattern the way a `Texture` face is.
        let sampler = texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy);
        let layouts = Layouts {
            frame: frame_layout,
            image: &image_layout,
            materials: material_layout,
            appearance: &appearance_layout,
        };
        let mut meshes = FileMeshes {
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
                &Resolved::default(),
            ),
            image_layout,
            appearance_layout,
            texture_max_size: quality.texture_max_size,
            sampler,
            images: Images::default(),
            opaque: Keyed::new("rbxview filemesh instances"),
            blended: Vec::new(),
            blended_index: HashMap::new(),
            order: Vec::new(),
        };
        meshes.rebuild(device, queue, resolved);
        meshes
    }

    /// Replaces every batch with `resolved`'s instances, keeping the
    /// pipelines and every upload the new scene asks for again: a mesh
    /// texture (see [`Images`]), a `SurfaceAppearance` map set (see
    /// [`appearance::Sets::rebuild`]) and a batch's converted vertex buffers
    /// — the same mesh under the same skin is the same bytes, so a batch
    /// keyed the same way as one the previous scene drew takes its geometry
    /// over instead of converting and uploading the mesh again.
    pub(super) fn rebuild(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resolved: &Resolved,
    ) {
        let binding = Binding {
            layout: &self.image_layout,
            sampler: &self.sampler,
            max_size: self.texture_max_size,
        };
        // Before the batches: a batch's skin indexes the sets, and the index
        // an appearance lands at is only known once they are re-ordered.
        self.appearances.rebuild(
            device,
            queue,
            Binding {
                layout: &self.appearance_layout,
                sampler: &self.sampler,
                max_size: self.texture_max_size,
            },
            resolved,
        );
        let mut spare: Vec<(GroupKey, Geometry)> =
            std::mem::replace(&mut self.opaque, Keyed::new("rbxview filemesh instances"))
                .into_groups()
                .into_iter()
                .map(|group| (group.key, group.extra))
                .chain(
                    std::mem::take(&mut self.blended)
                        .into_iter()
                        .map(|blended| (blended.key, blended.geometry)),
                )
                .collect();
        self.blended_index.clear();
        self.order.clear();

        for (key, group) in group_by_mesh_and_skin(resolved) {
            let Some(mesh) = resolved.meshes.get(&key.mesh) else {
                continue;
            };
            let Some(skin) = self.images.slot(device, queue, binding, resolved, &key) else {
                continue;
            };
            // A skin's index can move between two scenes (see `Skin`), so a
            // spare batch is only the same geometry if its skin still agrees.
            let mut geometry = || {
                take_spare(&mut spare, &key, |geometry| geometry.skin == skin)
                    .unwrap_or_else(|| build(device, mesh, skin))
            };

            let blends = blends(resolved, &key);
            let (see_through, still): (Vec<_>, Vec<_>) = group
                .into_iter()
                .partition(|instance| blends || instance.alpha < 1.0);
            if !still.is_empty() {
                let roster = Roster::from_iter(still.iter().map(|i| (i.referent, raw(i), ())));
                let geometry = geometry();
                self.opaque.add_group(device, key.clone(), geometry, roster);
            }
            if !see_through.is_empty() {
                for instance in &see_through {
                    self.blended_index
                        .insert(instance.referent, self.blended.len());
                }
                let items: Vec<_> = see_through
                    .iter()
                    .map(|i| (i.referent, center(i), raw(i)))
                    .collect();
                let geometry = geometry();
                self.blended
                    .push(Blended::new(device, key, geometry, items));
            }
        }
    }

    /// Re-views every mesh texture and `SurfaceAppearance` map at the new cap and
    /// anisotropy. Nothing is decoded, uploaded or re-batched.
    pub(super) fn set_quality(&mut self, device: &wgpu::Device, quality: &QualityProfile) {
        self.sampler = texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy);
        self.texture_max_size = quality.texture_max_size;
        self.images.rebind(
            device,
            Binding {
                layout: &self.image_layout,
                sampler: &self.sampler,
                max_size: quality.texture_max_size,
            },
        );
        self.appearances.rebind(
            device,
            Binding {
                layout: &self.appearance_layout,
                sampler: &self.sampler,
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
            queue.write_buffer(&blended.instances, 0, bytemuck::cast_slice(&instances));
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
        let batches = self.opaque.groups().iter().map(|group| Draw {
            geometry: &group.extra,
            instances: group.slots.buffer(),
            count: group.slots.count(),
        });
        self.run(pass, bindings, &self.opaque_pipelines, batches);
    }

    /// Draws the translucent meshes in the order [`FileMeshes::prepare`] worked
    /// out, after everything opaque in the frame.
    pub(super) fn draw_blended(&self, pass: &mut wgpu::RenderPass<'_>, bindings: Bindings<'_>) {
        let batches = self.order.iter().map(|&index| {
            let blended = &self.blended[index];
            Draw {
                geometry: &blended.geometry,
                instances: &blended.instances,
                count: blended.items.len() as u32,
            }
        });
        self.run(pass, bindings, &self.blended_pipelines, batches);
    }

    fn run<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        bindings: Bindings<'_>,
        pipelines: &Pipelines,
        batches: impl Iterator<Item = Draw<'a>>,
    ) {
        let mut bound: Option<&wgpu::RenderPipeline> = None;
        // A batch an edit emptied (see `FileMeshes::sync`) keeps its buffers
        // but has nothing to draw, and no reason to bind them.
        for batch in batches.filter(|batch| batch.count > 0) {
            let geometry = batch.geometry;
            let pipeline = pipelines.get(geometry.skin);
            if !bound.is_some_and(|previous| std::ptr::eq(previous, pipeline)) {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bindings.frame, &[]);
                pass.set_bind_group(1, bindings.materials, &[]);
                bound = Some(pipeline);
            }
            match geometry.skin {
                Skin::Plain => {}
                Skin::Image(slot) => {
                    pass.set_bind_group(2, &self.images.bind_groups[slot], &[]);
                }
                Skin::Appearance(slot) => {
                    pass.set_bind_group(2, &self.appearances.bind_groups[slot], &[]);
                }
            }

            pass.set_vertex_buffer(0, geometry.vertices.slice(..));
            pass.set_vertex_buffer(1, batch.instances.slice(..));
            // File mesh vertex counts aren't bounded the way the procedural
            // shapes are, so indices stay 32-bit rather than risking silent
            // truncation.
            pass.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..geometry.index_count, 0, 0..batch.count);
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

impl GroupKey {
    fn of(instance: &ResolvedInstance) -> Self {
        GroupKey {
            mesh: instance.mesh.clone(),
            texture: instance.texture.clone(),
            appearance: instance.appearance,
        }
    }
}

/// Groups resolved instances by (mesh, skin), preserving first-seen order so
/// batch construction is deterministic run to run.
fn group_by_mesh_and_skin(resolved: &Resolved) -> Vec<(GroupKey, Vec<&ResolvedInstance>)> {
    let mut order: Vec<GroupKey> = Vec::new();
    let mut groups: HashMap<GroupKey, Vec<&ResolvedInstance>> = HashMap::new();

    for instance in &resolved.instances {
        let key = GroupKey::of(instance);
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
fn build(device: &wgpu::Device, mesh: &rbx_mesh::Mesh, skin: Skin) -> Geometry {
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

    Geometry {
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
        skin,
    }
}
