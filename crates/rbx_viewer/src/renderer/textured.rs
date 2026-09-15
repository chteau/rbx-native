//! The second pass: `Decal` and `Texture` images projected back onto the very
//! surfaces they are pinned to, one instanced draw per (image, shape) pair.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::geometry::Meshes;
use super::mesh::Vertex;
use super::pipeline::{self, Surface, Target, DECAL_SHADER};
use super::texture;
use crate::quality::QualityProfile;
use crate::scene::ShapeKind;
use crate::textures::{FaceInstance, Group};

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x4,
    6 => Float32x3,
    7 => Float32x3,
    8 => Float32x3,
    9 => Float32x4,
    10 => Float32x4,
    11 => Uint32,
];

/// Depth bias to eliminate z-fighting between a decal and the surface it is
/// projected on, which are the same geometry drawn twice: the constant bias moves
/// the decal closer by a fixed depth-buffer step count, and the slope bias scales
/// with surface angle so small parts and huge baseplates behave alike.
///
/// Positive, not negative: reversed-Z (see camera.rs) makes closer mean a *bigger*
/// depth value, so pushing a decal toward the camera means adding to its depth
/// instead of subtracting from it.
const DEPTH_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState {
    constant: 16,
    slope_scale: 2.0,
    clamp: 0.0,
};

/// One face instance on the GPU: where the part stands, and how the image is
/// projected onto it — see `textured.wgsl` for how a fragment turns this into a
/// UV, and [`crate::textures::Projection`] for the projection itself.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct DecalRaw {
    model: [[f32; 4]; 4],
    face_normal: [f32; 3],
    u_axis: [f32; 3],
    v_axis: [f32; 3],
    uv_transform: [f32; 4],
    tint: [f32; 4],
    /// 1 on a wedge, whose slope belongs to the Front face however its normal
    /// classifies — the shader has no other way to tell one mesh from another.
    wedge: u32,
}

impl DecalRaw {
    fn new(face: &FaceInstance) -> Self {
        let projection = &face.projection;
        DecalRaw {
            model: face.model.to_cols_array_2d(),
            face_normal: projection.normal.to_array(),
            u_axis: projection.u.to_array(),
            v_axis: projection.v.to_array(),
            uv_transform: [
                projection.uv_scale[0],
                projection.uv_scale[1],
                projection.uv_offset[0],
                projection.uv_offset[1],
            ],
            tint: [face.tint[0], face.tint[1], face.tint[2], face.alpha],
            wedge: u32::from(face.kind == ShapeKind::Wedge),
        }
    }

    const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<DecalRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        }
    }
}

/// One draw call: every face instance in a scene sharing an image, a shape and a
/// pass.
struct Batch {
    image: usize,
    kind: ShapeKind,
    instances: wgpu::Buffer,
    instance_count: u32,
}

/// Both textured passes and the GPU state they draw from.
pub(super) struct Textured {
    opaque_pipeline: wgpu::RenderPipeline,
    blended_pipeline: wgpu::RenderPipeline,
    /// The images themselves, kept beside the bind groups that view them so a
    /// quality level can re-view them without re-uploading (see
    /// [`Textured::set_quality`]).
    uploads: Vec<texture::Uploaded>,
    image_layout: wgpu::BindGroupLayout,
    images: Vec<wgpu::BindGroup>,
    opaque: Vec<Batch>,
    blended: Vec<Batch>,
}

impl Textured {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        view_projection: &wgpu::BindGroupLayout,
        groups: &[Group],
        quality: &QualityProfile,
    ) -> Self {
        let image_layout = texture::layout(device);
        // Repeat, because a Texture's whole point is tiling past its own edges.
        let sampler = texture::sampler(device, wgpu::AddressMode::Repeat, quality.anisotropy);

        let mut uploads = Vec::with_capacity(groups.len());
        let mut images = Vec::with_capacity(groups.len());
        let mut opaque = Vec::new();
        let mut blended = Vec::new();
        for group in groups {
            let slot = images.len();
            let upload = texture::Uploaded::color(device, queue, &group.image);
            images.push(upload.bind(device, &image_layout, &sampler, quality.texture_max_size));
            uploads.push(upload);
            opaque.extend(batches(device, slot, &group.opaque));
            blended.extend(batches(device, slot, &group.blended));
        }

        let layouts = [Some(view_projection), Some(&image_layout)];
        Textured {
            opaque_pipeline: create(device, target, &layouts, false),
            blended_pipeline: create(device, target, &layouts, true),
            uploads,
            image_layout,
            images,
            opaque,
            blended,
        }
    }

    /// Re-views every decal at the new texture cap and anisotropy: one sampler
    /// and one bind group per image, with nothing decoded or uploaded again.
    pub(super) fn set_quality(&mut self, device: &wgpu::Device, quality: &QualityProfile) {
        let sampler = texture::sampler(device, wgpu::AddressMode::Repeat, quality.anisotropy);
        self.images = self
            .uploads
            .iter()
            .map(|upload| {
                upload.bind(
                    device,
                    &self.image_layout,
                    &sampler,
                    quality.texture_max_size,
                )
            })
            .collect();
    }

    /// Rebuilds both pipelines for a new sample count, which is the one quality
    /// knob baked into them.
    pub(super) fn set_target(
        &mut self,
        device: &wgpu::Device,
        target: Target,
        view_projection: &wgpu::BindGroupLayout,
    ) {
        let layouts = [Some(view_projection), Some(&self.image_layout)];
        self.opaque_pipeline = create(device, target, &layouts, false);
        self.blended_pipeline = create(device, target, &layouts, true);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.opaque.is_empty() && self.blended.is_empty()
    }

    /// Draws the opaque face instances; call before anything translucent in the
    /// frame.
    pub(super) fn draw_opaque(&self, pass: &mut wgpu::RenderPass<'_>, meshes: &Meshes) {
        self.draw(pass, meshes, &self.opaque_pipeline, &self.opaque);
    }

    /// Draws the translucent face instances with the depth buffer read-only, so
    /// they blend over whatever is already there in whichever order they come.
    pub(super) fn draw_blended(&self, pass: &mut wgpu::RenderPass<'_>, meshes: &Meshes) {
        self.draw(pass, meshes, &self.blended_pipeline, &self.blended);
    }

    fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        meshes: &Meshes,
        pipeline: &wgpu::RenderPipeline,
        batches: &[Batch],
    ) {
        if batches.is_empty() {
            return;
        }

        pass.set_pipeline(pipeline);
        for batch in batches {
            let Some(mesh) = meshes.get(batch.kind) else {
                continue;
            };
            pass.set_bind_group(1, &self.images[batch.image], &[]);
            pass.set_vertex_buffer(1, batch.instances.slice(..));
            mesh.draw(pass, batch.instance_count);
        }
    }
}

/// Splits the face instances sharing one image into one batch per shape kind,
/// since each kind is a different mesh to instance.
fn batches(device: &wgpu::Device, image: usize, faces: &[FaceInstance]) -> Vec<Batch> {
    let mut grouped: Vec<(ShapeKind, Vec<DecalRaw>)> = Vec::new();
    for face in faces {
        let raw = DecalRaw::new(face);
        match grouped.iter_mut().find(|(kind, _)| *kind == face.kind) {
            Some((_, instances)) => instances.push(raw),
            None => grouped.push((face.kind, vec![raw])),
        }
    }

    grouped
        .into_iter()
        .map(|(kind, instances)| Batch {
            image,
            kind,
            instance_count: instances.len() as u32,
            instances: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview decal instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        })
        .collect()
}

fn create(
    device: &wgpu::Device,
    target: Target,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    blended: bool,
) -> wgpu::RenderPipeline {
    let buffers = [Some(Vertex::layout()), Some(DecalRaw::layout())];

    pipeline::surface(
        device,
        target,
        &Surface {
            translucent: blended,
            bias: DEPTH_BIAS,
            compare: wgpu::CompareFunction::GreaterEqual,
            ..Surface::new("rbxview decals", DECAL_SHADER, bind_group_layouts, &buffers)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Nothing but their order links the WGSL struct to this layout, so a field
    // added to one alone reads the neighbouring attribute instead of failing to
    // compile.
    #[test]
    fn the_shader_reads_the_wedge_flag_the_layout_supplies() {
        assert!(DECAL_SHADER.contains("@location(11) wedge: u32"));
        assert_eq!(INSTANCE_ATTRIBUTES.len(), 10);
        assert_eq!(INSTANCE_ATTRIBUTES[9].shader_location, 11);
        assert_eq!(
            INSTANCE_ATTRIBUTES[9].offset + INSTANCE_ATTRIBUTES[9].format.size(),
            std::mem::size_of::<DecalRaw>() as wgpu::BufferAddress
        );
    }
}
