//! `Terrain.Decoration`'s animated grass: blades on the Grass parts of the
//! solid surface, instanced from one blade mesh (`grass.wgsl`).
//!
//! A re-mesh keeps only each chunk's grassy triangles ([`scatter::patches`]);
//! blades are scattered from them lazily, one chunk a frame and nearest
//! first, for the chunks in view within the quality level's reach of the
//! eye, and dropped again once the eye moves away.
//! Every chunk's blades are generated at full density and sorted by rank per
//! tile, so the level's density and the thinning with distance are only how
//! much of each tile is drawn: switching level re-scatters nothing.

mod scatter;

use std::collections::HashMap;

use glam::Vec3;
use rbx_terrain::{ChunkKey, Material, CHUNK, VOXEL_STUDS};

use super::super::pipeline::{self, Surface, Target, GRASS_SHADER};
use super::buffer;
use crate::quality::QualityProfile;
use crate::scene::terrain::Terrain;
use scatter::{density_at, BladeRaw, Tile};
pub(super) use scatter::{patches, Patch};

/// The blade: three tapering segments, a strip of two-vertex rows and a tip.
const BLADE: [[f32; 2]; 7] = [
    [-1.0, 0.0],
    [1.0, 0.0],
    [-1.0, 0.4],
    [1.0, 0.4],
    [-1.0, 0.75],
    [1.0, 0.75],
    [0.0, 1.0],
];
const BLADE_INDICES: [u16; 15] = [0, 1, 2, 1, 3, 2, 2, 3, 4, 3, 5, 4, 4, 5, 6];

/// The longest blade at `GrassLength` 1, in studs: Roblox's default of 0.7
/// gives knee-high blades of a couple of studs.
const LONGEST_BLADE: f32 = 3.0;
/// A tile's blades never stand taller than this over their roots.
const HEADROOM: f32 = LONGEST_BLADE * 1.25;
/// Whole flutter cycles per shimmer period in still air (see
/// `pipeline::shimmer_phase`).
const STILL_FLUTTER: f32 = 5.0;
/// Chunks scattered per frame at most: one costs 3 to 6 ms of CPU, and a
/// whole reach's worth at once (16 chunks, 45 ms) is a visible hitch.
const SCATTERS_PER_FRAME: usize = 1;

/// What the Grass tint is scaled by at a blade's root and tip: dark in the
/// shade of the carpet, a little greener than the tint at the top.
const ROOT_SHADE: [f32; 3] = [0.3, 0.42, 0.25];
const TIP_SHADE: [f32; 3] = [0.8, 1.08, 0.58];

/// `GrassLook` in `grass.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct LookRaw {
    root: [f32; 4],
    tip: [f32; 4],
    wind: [f32; 4],
    params: [f32; 4],
}

struct Blades {
    instances: Option<wgpu::Buffer>,
    tiles: Vec<Tile>,
}

pub(in crate::renderer) struct Grass {
    pipeline: wgpu::RenderPipeline,
    look_layout: wgpu::BindGroupLayout,
    look: wgpu::Buffer,
    look_group: wgpu::BindGroup,
    blade: wgpu::Buffer,
    blade_indices: wgpu::Buffer,
    /// Each chunk's grassy triangles, for every chunk that has any.
    patches: HashMap<ChunkKey, Vec<Patch>>,
    /// The chunks near enough the eye to have blades right now.
    blades: HashMap<ChunkKey, Blades>,
    raw: LookRaw,
    enabled: bool,
    eye: Vec3,
    /// Whether the last [`Grass::prepare`] saw a blade on screen, or chunks
    /// in view still waiting for theirs.
    shown: bool,
}

/// The pipeline layout's own half: group 1, the look.
fn look_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview grass look"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn build(
    device: &wgpu::Device,
    target: Target,
    (frame, look): (&wgpu::BindGroupLayout, &wgpu::BindGroupLayout),
) -> wgpu::RenderPipeline {
    let layouts = [Some(frame), Some(look)];
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BladeRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![1 => Float32x3, 2 => Unorm8x4],
        }),
    ];
    // Both faces: a blade is a single sheet.
    let surface = Surface {
        cull: None,
        ..Surface::new("rbxview grass", GRASS_SHADER, &layouts, &buffers)
    };
    pipeline::surface(device, target, &surface)
}

impl Grass {
    pub(super) fn new(
        device: &wgpu::Device,
        target: Target,
        frame: &wgpu::BindGroupLayout,
    ) -> Self {
        let look_layout = look_layout(device);
        let look = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview grass look"),
            size: std::mem::size_of::<LookRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Grass {
            pipeline: build(device, target, (frame, &look_layout)),
            look_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rbxview grass look"),
                layout: &look_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: look.as_entire_binding(),
                }],
            }),
            look_layout,
            look,
            blade: buffer(
                device,
                "rbxview grass blade",
                &BLADE,
                wgpu::BufferUsages::VERTEX,
            ),
            blade_indices: buffer(
                device,
                "rbxview grass blade indices",
                &BLADE_INDICES,
                wgpu::BufferUsages::INDEX,
            ),
            patches: HashMap::new(),
            blades: HashMap::new(),
            raw: LookRaw::default(),
            enabled: false,
            eye: Vec3::ZERO,
            shown: false,
        }
    }

    pub(super) fn set_target(
        &mut self,
        device: &wgpu::Device,
        target: Target,
        frame: &wgpu::BindGroupLayout,
    ) {
        self.pipeline = build(device, target, (frame, &self.look_layout));
    }

    pub(super) fn clear(&mut self) {
        self.patches.clear();
        self.blades.clear();
    }

    /// Takes `key`'s freshly meshed grassy triangles; its blades, if it had
    /// any, are scattered again the next frame.
    pub(super) fn set_patches(&mut self, key: ChunkKey, patches: Vec<Patch>) {
        self.blades.remove(&key);
        if patches.is_empty() {
            self.patches.remove(&key);
        } else {
            self.patches.insert(key, patches);
        }
    }

    /// The terrain's grass settings: the Grass tint, `GrassLength`,
    /// `Decoration` and the wind.
    pub(super) fn set_look(&mut self, terrain: &Terrain) {
        let grass = terrain.grass;
        self.enabled = grass.enabled;
        // The tint alone reads yellow: on the ground it multiplies the grass
        // pack's own green, which the blades have no texture to supply.
        let tint = terrain.tint(Material::Grass);
        let shade = |scale: [f32; 3]| [0, 1, 2].map(|i| tint[i] * scale[i]);
        let [r, g, b] = shade(ROOT_SHADE);
        self.raw.root = [r, g, b, grass.length * LONGEST_BLADE];
        let [r, g, b] = shade(TIP_SHADE);
        self.raw.tip = [r, g, b, self.raw.tip[3]];
        // Roblox sways grass gently in still air; `GlobalWind` (studs per
        // second) picks the direction and strengthens the lean and the beat.
        let horizontal = glam::Vec2::new(grass.wind.x, grass.wind.z);
        let speed = horizontal.length();
        let direction = if speed > 1e-3 {
            horizontal / speed
        } else {
            glam::Vec2::new(0.8, 0.6)
        };
        let strength = (speed / 50.0).min(1.0);
        self.raw.wind = [direction.x, 0.0, direction.y, 0.3 + 0.9 * strength];
        let flutter = (STILL_FLUTTER + 6.0 * strength).round();
        self.raw.params[1] = flutter;
        self.raw.params[2] = (flutter / 3.0).round().max(1.0);
    }

    /// Takes effect from the next frame; nothing is re-scattered.
    pub(in crate::renderer) fn set_quality(&mut self, quality: &QualityProfile) {
        self.raw.params[0] = quality.grass_distance;
        self.raw.tip[3] = quality.grass_density;
    }

    fn reach(&self) -> f32 {
        if self.enabled && self.raw.tip[3] > 0.0 {
            self.raw.params[0]
        } else {
            0.0
        }
    }

    /// Whether a blade is on screen, which sways and so keeps the frame
    /// animating; so does grass in view still being scattered.
    pub(in crate::renderer) fn is_drawn(&self) -> bool {
        self.shown
    }

    /// Scatters the nearest chunk in view that came within reach of
    /// `eye`, drops the ones well out of it, and writes the look.
    pub(in crate::renderer) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        eye: Vec3,
        visible: impl Fn(Vec3, f32) -> bool,
    ) {
        self.eye = eye;
        self.shown = false;
        let reach = self.reach();
        if reach <= 0.0 {
            self.blades.clear();
            return;
        }
        // Kept a little past the reach and out of view, so a camera hovering
        // at the edge or turning about does not scatter the same chunk again.
        self.blades
            .retain(|key, _| chunk_distance(*key, eye) <= reach * 1.25);
        let missing = self
            .patches
            .keys()
            .filter(|key| !self.blades.contains_key(key));
        let (due, more) = due(missing.copied(), eye, reach, &visible);
        for key in due {
            self.blades
                .insert(key, upload(device, key, &self.patches[&key]));
        }
        self.shown = more
            || self
                .blades
                .values()
                .flat_map(|b| &b.tiles)
                .any(|tile| !tile.blades.is_empty() && self.share(tile, &visible) > 0.0);
        queue.write_buffer(&self.look, 0, bytemuck::bytes_of(&self.raw));
    }

    /// The share of `tile`'s blades drawn: none out of view or past the reach.
    fn share(&self, tile: &Tile, visible: &impl Fn(Vec3, f32) -> bool) -> f32 {
        let max = tile.max + Vec3::Y * HEADROOM;
        let center = (tile.min + max) * 0.5;
        if !visible(center, (max - tile.min).length() * 0.5) {
            return 0.0;
        }
        let nearest = self.eye.clamp(tile.min, max).distance(self.eye);
        density_at(nearest, self.reach(), self.raw.tip[3])
    }

    /// The blades of every visible tile, as many of each as the distance
    /// wants. Rebinds group 1, so the caller rebinds before drawing more.
    pub(in crate::renderer) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        frame: &wgpu::BindGroup,
        visible: impl Fn(Vec3, f32) -> bool,
    ) {
        let reach = self.reach();
        if reach <= 0.0 || self.blades.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.look_group, &[]);
        pass.set_vertex_buffer(0, self.blade.slice(..));
        pass.set_index_buffer(self.blade_indices.slice(..), wgpu::IndexFormat::Uint16);
        for blades in self.blades.values() {
            let Some(instances) = &blades.instances else {
                continue;
            };
            pass.set_vertex_buffer(1, instances.slice(..));
            for tile in &blades.tiles {
                let share = self.share(tile, &visible);
                let count = (tile.blades.len() as f32 * share).ceil() as u32;
                if count > 0 {
                    let start = tile.blades.start;
                    pass.draw_indexed(0..BLADE_INDICES.len() as u32, 0, start..start + count);
                }
            }
        }
    }
}

fn chunk_origin(key: ChunkKey) -> Vec3 {
    Vec3::from(key.origin().map(|v| v as f32 * VOXEL_STUDS))
}

/// From `eye` to the nearest point of the chunk's box.
fn chunk_distance(key: ChunkKey, eye: Vec3) -> f32 {
    let min = chunk_origin(key);
    let max = min + Vec3::splat(CHUNK as f32 * VOXEL_STUDS);
    eye.clamp(min, max).distance(eye)
}

/// The chunks to scatter this frame out of the `missing` ones: the nearest
/// [`SCATTERS_PER_FRAME`] in view within `reach` of `eye`, and whether more
/// such chunks are left for the frames after.
fn due(
    missing: impl Iterator<Item = ChunkKey>,
    eye: Vec3,
    reach: f32,
    visible: impl Fn(Vec3, f32) -> bool,
) -> (Vec<ChunkKey>, bool) {
    let half = Vec3::splat(CHUNK as f32 * VOXEL_STUDS * 0.5);
    let mut due: Vec<(f32, ChunkKey)> = missing
        .map(|key| (chunk_distance(key, eye), key))
        .filter(|(studs, key)| *studs <= reach && visible(chunk_origin(*key) + half, half.length()))
        .collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    let more = due.len() > SCATTERS_PER_FRAME;
    due.truncate(SCATTERS_PER_FRAME);
    (due.into_iter().map(|(_, key)| key).collect(), more)
}

fn upload(device: &wgpu::Device, key: ChunkKey, patches: &[Patch]) -> Blades {
    let (blades, tiles) = scatter::scatter(patches, chunk_origin(key));
    Blades {
        instances: (!blades.is_empty()).then(|| {
            buffer(
                device,
                "rbxview grass blades",
                &blades,
                wgpu::BufferUsages::VERTEX,
            )
        }),
        tiles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(x: i32) -> ChunkKey {
        ChunkKey { x, y: 0, z: 0 }
    }

    #[test]
    fn only_the_nearest_chunks_in_view_and_reach_are_scattered() {
        let span = CHUNK as f32 * VOXEL_STUDS;
        // The eye sits in chunk 0 looking down +X; chunk -1 is behind it.
        let eye = Vec3::splat(span * 0.5);
        let ahead = |center: Vec3, _: f32| center.x >= 0.0;
        let (picked, more) = due(
            [3, -1, 2, 0, 1, 9].map(key).into_iter(),
            eye,
            span * 4.0,
            ahead,
        );
        assert_eq!(picked, [0, 1, 2, 3].map(key)[..SCATTERS_PER_FRAME]);
        assert!(more, "the rest are left for later frames");
        let (picked, more) = due([-1, 9].map(key).into_iter(), eye, span * 4.0, ahead);
        assert!(
            picked.is_empty() && !more,
            "behind the eye or past the reach"
        );
    }
}
