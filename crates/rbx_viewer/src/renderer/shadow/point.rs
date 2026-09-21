//! Six-face shadow maps for `PointLight`s — the one local light that has no
//! cone to fit a single map to, and so the one `shadow::local` cannot serve.
//!
//! A point light is spelled as a cone with no axis at all (see
//! `crate::lighting::local`), which is what [`select`] filters on. Each one
//! chosen gets six 90-degree perspectives — one per signed axis — into six
//! consecutive layers of a depth array of their own; the shader picks the
//! face a fragment belongs to from the major axis of the light-to-fragment
//! direction and reads that layer with that face's own matrix, which is the
//! same projection-and-compare the cone lights already use rather than a
//! second set of cube-map conventions to get wrong.

use glam::camera::rh::proj::directx::perspective;
use glam::camera::rh::view::look_to_mat4;
use glam::{Mat4, Vec3};

use crate::lighting::LocalLight;

use super::local::NEAR_STUDS;

/// One cube: +X, -X, +Y, -Y, +Z, -Z, in the order the shader's own major
/// -axis test numbers them.
pub(in crate::renderer) const FACES: usize = 6;

/// Each face covers a quadrant of the sphere, which is 90 degrees exactly —
/// widened by a hair so a PCF tap taken right at a face boundary still lands
/// inside that face's map instead of off its edge, where the lookup would
/// read "lit" and leave a seam along the boundary.
const FACE_FOV_DEGREES: f32 = 92.0;

/// The direction each face looks along, and the up vector its view is built
/// with. Only consistency matters, not any cube-map convention: the shader
/// projects through the very matrix built here rather than sampling a cube.
/// Side of one face. Half the cone lights' own map: a point light spends
/// six of these where a cone light spends one, and halving the side is what
/// keeps a cube at the same memory a single cone map costs plus half again,
/// rather than at six times it.
const SIDE: u32 = 512;

const AXES: [(Vec3, Vec3); FACES] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::Z),
    (Vec3::NEG_Y, Vec3::Z),
    (Vec3::Z, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y),
];

/// One point light chosen to cast this frame.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::renderer) struct Selected {
    /// Position in the slice [`select`] was given — the index its record in
    /// the light-shadow buffer sits at, exactly as `local::Selected`'s is.
    pub(in crate::renderer) index: usize,
    pub(in crate::renderer) faces: [Mat4; FACES],
}

/// Every enabled, axis-less, `Shadows = true` light in `lights`, nearest
/// `camera` first and truncated to `cap` *cubes* — not layers; each one
/// selected here spends [`FACES`] of them.
///
/// Sorted by distance to the camera for the same reason the cone lights are:
/// it is the ones a frame is actually looking at that earn a map.
pub(in crate::renderer) fn select(
    lights: &[LocalLight],
    camera: Vec3,
    cap: usize,
) -> Vec<Selected> {
    let mut candidates: Vec<(usize, &LocalLight)> = lights
        .iter()
        .enumerate()
        .filter(|(_, light)| light.shadows && light.direction == Vec3::ZERO)
        .collect();

    candidates.sort_by(|(_, a), (_, b)| {
        let distance = |light: &LocalLight| light.position.distance_squared(camera);
        distance(a).total_cmp(&distance(b))
    });
    candidates.truncate(cap);

    candidates
        .into_iter()
        .map(|(index, light)| Selected {
            index,
            faces: faces_of(light),
        })
        .collect()
}

/// The six perspectives one light is drawn from, in [`AXES`] order.
///
/// Standard (not reversed) depth and the same near plane the cone lights
/// use, so both kinds of map read through one comparison sampler and one
/// bias.
fn faces_of(light: &LocalLight) -> [Mat4; FACES] {
    let far = light.range.max(NEAR_STUDS + 0.01);
    let fov = FACE_FOV_DEGREES.to_radians();
    AXES.map(|(direction, up)| {
        perspective(fov, 1.0, NEAR_STUDS, far) * look_to_mat4(light.position, direction, up)
    })
}

/// The `PointLight` cube array: six layers per cube, each drawn into on its
/// own and all sampled through one array view — see
/// `renderer::shadow::point` for why this is an array of perspectives
/// rather than a cube map.
pub(in crate::renderer) fn map(
    device: &wgpu::Device,
    cubes: usize,
) -> (wgpu::TextureView, Vec<wgpu::TextureView>) {
    let layers = (cubes * FACES).max(1) as u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rbxview point shadow map"),
        size: wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: super::FORMAT,
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
/// [`map`]'s array. Never empty, for the same reason the light buffer
/// itself never is: a storage binding has to point at something.
pub(in crate::renderer) fn faces_buffer(device: &wgpu::Device, cubes: usize) -> wgpu::Buffer {
    let layers = (cubes * FACES).max(FACES);
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rbxview point shadow faces"),
        size: super::MATRIX_SIZE * layers as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// An empty matrix array for a pass that draws no shadows at all — a
/// `ViewportFrame`'s own, which still has to bind something at the slot
/// (see `renderer::gui::viewport`).
pub(in crate::renderer) fn faces_stand_in(device: &wgpu::Device) -> wgpu::Buffer {
    use wgpu::util::DeviceExt as _;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rbxview point shadow faces stand-in"),
        contents: bytemuck::cast_slice(&[[[0.0f32; 4]; 4]; FACES]),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

#[cfg(test)]
#[path = "point/tests.rs"]
mod tests;
