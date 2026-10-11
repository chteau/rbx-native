//! Skinned meshes deformed on the GPU: a compute pass that blends each rig's
//! rest vertices through its bone matrices straight into every vertex buffer
//! drawing it — the main mesh pass, the shadow casters, and the highlight and
//! selection masks — so all of them follow the same pose.
//!
//! The rest vertices and the influences are uploaded once per rig; a pose
//! change writes only the bone matrices (two `mat4` per bone). A buffer is
//! re-skinned when its rig's pose changed since it was last written, or when
//! it is new (a rebuild or a patch made it from the rest mesh).
//!
//! WebGL2 has no compute shaders and no storage buffers: there [`supported`]
//! says no, and the scene keeps deforming on the CPU (see
//! `scene::filemesh::Resolved::skins_on_gpu`).

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat3, Mat4};
use rbx_assets::AssetRef;
use wgpu::util::DeviceExt;

const WORKGROUP: u32 = 64;
/// The most workgroups one dispatch dimension takes on every backend.
const MAX_GROUPS: u32 = 65_535;
const NONE: u32 = u32::MAX;

/// Whether `device` can skin on the GPU: compute shaders and four storage
/// buffers in one stage.
pub(super) fn supported(device: &wgpu::Device) -> bool {
    let limits = device.limits();
    limits.max_storage_buffers_per_shader_stage >= 4
        && limits.max_compute_workgroups_per_dimension >= MAX_GROUPS
        && limits.max_compute_invocations_per_workgroup >= WORKGROUP
}

/// The usage every vertex buffer a skinned mesh may draw from needs on top
/// of its own, so the pass can write it.
pub(super) fn vertex_usage(device: &wgpu::Device) -> wgpu::BufferUsages {
    match supported(device) {
        true => wgpu::BufferUsages::STORAGE,
        false => wgpu::BufferUsages::empty(),
    }
}

/// Where a vertex buffer keeps what the pass writes, in floats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Layout {
    pub(super) stride: u32,
    pub(super) normal: Option<u32>,
    pub(super) tangent: Option<u32>,
}

impl Layout {
    /// Positions only: the shadow casters' and the masks' geometry.
    pub(super) const POSITIONS: Layout = Layout {
        stride: 3,
        normal: None,
        tangent: None,
    };
}

/// One rig's GPU half: its rest mesh, its influences and its pose.
struct Rig {
    source: Arc<rbx_mesh::SkinnedMesh>,
    rest: wgpu::Buffer,
    influences: wgpu::Buffer,
    palette: wgpu::Buffer,
    count: u32,
    /// Which upload of the buffers above this is, for a target's bind group
    /// to notice it was made against older ones.
    build: u64,
    /// Which pose the palette holds.
    pose: u64,
}

/// One vertex buffer drawing a rig.
struct Target {
    build: u64,
    /// The rig pose last written into it.
    pose: u64,
    layout: Layout,
    bind_group: wgpu::BindGroup,
    seen: bool,
}

pub(super) struct Skinner {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    rigs: HashMap<AssetRef, Rig>,
    targets: HashMap<wgpu::Buffer, Target>,
    /// Counter behind every `build` and `pose`.
    serial: u64,
}

impl Skinner {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rbxview skinning"),
            entries: &[
                storage(0, true),
                storage(1, true),
                storage(2, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(4, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rbxview skinning"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = crate::gpu::shader(device, "rbxview skinning", include_str!("skinning.wgsl"));
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("rbxview skinning"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Skinner {
            pipeline,
            layout,
            rigs: HashMap::new(),
            targets: HashMap::new(),
            serial: 0,
        }
    }

    fn next(&mut self) -> u64 {
        self.serial += 1;
        self.serial
    }

    /// Takes the rig drawn under `key` at the pose `palette` (one native-space
    /// matrix per global bone): its rest buffers are uploaded again only when
    /// `source` is another mesh.
    pub(super) fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: &AssetRef,
        source: &Arc<rbx_mesh::SkinnedMesh>,
        palette: &[Mat4],
    ) {
        let bytes = palette_bytes(source, palette);
        let pose = self.next();
        if let Some(rig) = self.rigs.get_mut(key) {
            if Arc::ptr_eq(&rig.source, source) && rig.palette.size() == bytes.len() as u64 {
                queue.write_buffer(&rig.palette, 0, &bytes);
                rig.pose = pose;
                return;
            }
        }
        let count = source.mesh.vertices.len() as u32;
        if count == 0 {
            self.rigs.remove(key);
            return;
        }
        let build = self.next();
        let rig = Rig {
            source: source.clone(),
            rest: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview skinning rest"),
                contents: &rest_bytes(&source.mesh),
                usage: wgpu::BufferUsages::STORAGE,
            }),
            influences: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview skinning influences"),
                contents: &influence_bytes(source),
                usage: wgpu::BufferUsages::STORAGE,
            }),
            palette: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview skinning palette"),
                contents: &bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            }),
            count,
            build,
            pose,
        };
        self.rigs.insert(key.clone(), rig);
    }

    /// Forgets the rig drawn under `key`.
    pub(super) fn remove(&mut self, key: &AssetRef) {
        self.rigs.remove(key);
    }

    /// Keeps only the rigs `keep` names.
    pub(super) fn retain(&mut self, keep: impl Fn(&AssetRef) -> bool) {
        self.rigs.retain(|key, _| keep(key));
    }

    /// Writes the current pose into every one of `targets` (a rig key, the
    /// vertex buffer drawing it and its layout) that does not hold it yet,
    /// in one compute pass submitted on its own, ahead of the frame that
    /// draws them. Buffers no longer offered are let go.
    pub(super) fn run<'a>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        targets: impl IntoIterator<Item = (&'a AssetRef, &'a wgpu::Buffer, Layout)>,
    ) {
        if self.rigs.is_empty() {
            self.targets.clear();
            return;
        }
        for target in self.targets.values_mut() {
            target.seen = false;
        }
        let mut owed: Vec<(wgpu::Buffer, u32)> = Vec::new();
        for (key, buffer, layout) in targets {
            let Some(rig) = self.rigs.get(key) else {
                continue;
            };
            let floats = u64::from(rig.count) * u64::from(layout.stride);
            if buffer.size() != floats * 4 {
                continue;
            }
            let current = self
                .targets
                .get(buffer)
                .is_some_and(|target| target.build == rig.build && target.layout == layout);
            if !current {
                let bind_group = bind_group(device, &self.layout, rig, buffer, layout);
                self.targets.insert(
                    buffer.clone(),
                    Target {
                        build: rig.build,
                        pose: 0,
                        layout,
                        bind_group,
                        seen: false,
                    },
                );
            }
            let Some(target) = self.targets.get_mut(buffer) else {
                continue;
            };
            target.seen = true;
            if target.pose != rig.pose {
                target.pose = rig.pose;
                owed.push((buffer.clone(), rig.count));
            }
        }
        self.targets.retain(|_, target| target.seen);
        if owed.is_empty() {
            return;
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rbxview skinning"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("rbxview skinning"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (buffer, count) in &owed {
                let Some(target) = self.targets.get(buffer) else {
                    continue;
                };
                let (x, y) = grid(*count);
                pass.set_bind_group(0, &target.bind_group, &[]);
                pass.dispatch_workgroups(x, y, 1);
            }
        }
        queue.submit([encoder.finish()]);
    }
}

/// The dispatch grid for `count` vertices: rows of at most [`MAX_GROUPS`]
/// workgroups.
fn grid(count: u32) -> (u32, u32) {
    let groups = count.div_ceil(WORKGROUP).max(1);
    let x = groups.min(MAX_GROUPS);
    (x, groups.div_ceil(x))
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    rig: &Rig,
    buffer: &wgpu::Buffer,
    target: Layout,
) -> wgpu::BindGroup {
    let (x, _) = grid(rig.count);
    let params = [
        rig.count,
        target.stride,
        target.normal.unwrap_or(NONE),
        target.tangent.unwrap_or(NONE),
        x * WORKGROUP,
        0,
        0,
        0,
    ];
    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rbxview skinning params"),
        contents: bytemuck::cast_slice(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rbxview skinning"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: rig.rest.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: rig.influences.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: rig.palette.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: buffer.as_entire_binding(),
            },
        ],
    })
}

/// Position, normal and tangent of every vertex, a `vec4` each.
fn rest_bytes(mesh: &rbx_mesh::Mesh) -> Vec<u8> {
    let tangents = super::filemesh::mesh_tangents(mesh);
    let mut floats = Vec::with_capacity(mesh.vertices.len() * 12);
    for (vertex, tangent) in mesh.vertices.iter().zip(tangents) {
        let [px, py, pz] = vertex.position;
        let [nx, ny, nz] = vertex.normal;
        floats.extend_from_slice(&[px, py, pz, 1.0, nx, ny, nz, 0.0]);
        floats.extend_from_slice(&tangent);
    }
    bytemuck::cast_slice(&floats).to_vec()
}

/// Every vertex's four bones and weights. A bone past the palette is a bone
/// at rest, as in `deform`: it points at the identity pair appended last.
fn influence_bytes(source: &rbx_mesh::SkinnedMesh) -> Vec<u8> {
    let count = source.mesh.vertices.len();
    let bones = source.skin.as_ref().map_or(0, |skin| skin.bones.len());
    let mut words: Vec<u32> = Vec::with_capacity(count * 8);
    for index in 0..count {
        let influence = source
            .skin
            .as_ref()
            .and_then(|skin| skin.influences.get(index));
        let (mut slots, mut weights) = ([bones as u32; 4], [0.0f32; 4]);
        if let Some(influence) = influence {
            for k in 0..4 {
                let weight = influence.weights[k];
                if weight > 0.0 {
                    let bone = usize::from(influence.bones[k]);
                    slots[k] = bone.min(bones) as u32;
                    weights[k] = weight;
                }
            }
        }
        words.extend_from_slice(&slots);
        words.extend(weights.map(f32::to_bits));
    }
    bytemuck::cast_slice(&words).to_vec()
}

/// Each bone's matrix and its normal matrix, then the identity pair a bone
/// past the palette reads.
fn palette_bytes(source: &rbx_mesh::SkinnedMesh, palette: &[Mat4]) -> Vec<u8> {
    let bones = source.skin.as_ref().map_or(0, |skin| skin.bones.len());
    let mut floats = Vec::with_capacity((bones + 1) * 32);
    for bone in 0..bones {
        let matrix = palette.get(bone).copied().unwrap_or(Mat4::IDENTITY);
        let normal = Mat4::from_mat3(Mat3::from_mat4(matrix).inverse().transpose());
        floats.extend_from_slice(&matrix.to_cols_array());
        floats.extend_from_slice(&normal.to_cols_array());
    }
    floats.extend_from_slice(&Mat4::IDENTITY.to_cols_array());
    floats.extend_from_slice(&Mat4::IDENTITY.to_cols_array());
    bytemuck::cast_slice(&floats).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_covers_every_vertex() {
        assert_eq!(grid(1), (1, 1));
        assert_eq!(grid(64), (1, 1));
        assert_eq!(grid(65), (2, 1));
        let (x, y) = grid(MAX_GROUPS * WORKGROUP + 1);
        assert!(
            u64::from(x) * u64::from(y) * u64::from(WORKGROUP) > u64::from(MAX_GROUPS * WORKGROUP)
        );
    }
}
