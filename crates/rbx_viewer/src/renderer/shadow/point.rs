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
