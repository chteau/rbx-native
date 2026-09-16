//! The local light storage buffer: [`crate::lighting::LocalLight`] packed the
//! way `lighting.wgsl` reads it.
//!
//! Written once, at `Renderer::new`: a `PointLight` never moves in this viewer,
//! so nothing here is per-frame.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::lighting::LocalLight;

/// One light, as four `vec4`s.
///
/// All-`vec4` for the same reason the lighting uniform is (see [`super`]): the
/// layout is then the same whatever alignment rules the backend applies, and the
/// two declarations cannot drift apart unnoticed.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
pub(super) struct LocalLightRaw {
    /// xyz: world position. w: `Range`, where the falloff reaches zero.
    position_range: [f32; 4],
    /// rgb: linear radiance inside the near field. w: how far that field
    /// reaches, which is 0 on anything but a `SurfaceLight`.
    color_near: [f32; 4],
    /// xyz: unit cone axis, zero for a light that shines everywhere.
    /// w: cosine of the cone's half-angle.
    direction_cone: [f32; 4],
    /// x: cosine of the inner half-angle the edge is smoothed from, strictly
    /// greater than `direction_cone.w`. yzw: padding the `vec4` needs anyway.
    cone_inner: [f32; 4],
}

impl LocalLightRaw {
    fn new(light: &LocalLight) -> Self {
        LocalLightRaw {
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            color_near: [light.color.x, light.color.y, light.color.z, light.near],
            direction_cone: [
                light.direction.x,
                light.direction.y,
                light.direction.z,
                light.cos_outer,
            ],
            cone_inner: [light.cos_inner, 0.0, 0.0, 0.0],
        }
    }
}

/// Never empty: wgpu refuses a zero-sized binding, and the shader reads the
/// light count off the lighting uniform rather than the buffer's length, so a
/// scene with no lights uploads one zeroed entry nothing ever looks at.
fn pack(lights: &[LocalLight]) -> Vec<LocalLightRaw> {
    let mut packed: Vec<LocalLightRaw> = lights.iter().map(LocalLightRaw::new).collect();
    if packed.is_empty() {
        packed.push(LocalLightRaw::default());
    }
    packed
}

/// Packs every light into one storage buffer.
pub(in crate::renderer) fn buffer(device: &wgpu::Device, lights: &[LocalLight]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rbxview local lights"),
        contents: bytemuck::cast_slice(&pack(lights)),
        // COPY_DST: written afterwards by `write`, for a `Light` property
        // edit's fast path (see `Renderer::set_lights`).
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    })
}

/// Rewrites an existing buffer in place, for a `Light` edit that changed
/// neither the count nor which lights the level allows (see
/// `Renderer::set_lights`): same byte length in, so no bind group that
/// already points at `buffer` goes stale.
pub(in crate::renderer) fn write(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    lights: &[LocalLight],
) {
    queue.write_buffer(buffer, 0, bytemuck::cast_slice(&pack(lights)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    /// What `LocalLight` in lighting.wgsl declares, and the alignment WGSL gives
    /// a struct of `vec4`s in a storage array.
    #[test]
    fn the_packed_light_is_four_vec4s_aligned_to_one() {
        assert_eq!(std::mem::size_of::<LocalLightRaw>(), 4 * 16);
        assert_eq!(std::mem::align_of::<LocalLightRaw>() % 4, 0);
        // A stride that is not a multiple of 16 would make the shader read the
        // second light straddling two entries.
        assert_eq!(std::mem::size_of::<LocalLightRaw>() % 16, 0);
    }

    #[test]
    fn every_field_lands_in_the_slot_the_shader_reads_it_from() {
        let light = LocalLight {
            position: Vec3::new(1.0, 2.0, 3.0),
            color: Vec3::new(0.1, 0.2, 0.3),
            range: 12.0,
            near: 1.5,
            direction: -Vec3::Y,
            cos_outer: 0.25,
            cos_inner: 0.5,
            shadows: true,
        };

        let raw = LocalLightRaw::new(&light);

        assert_eq!(raw.position_range, [1.0, 2.0, 3.0, 12.0]);
        assert_eq!(raw.color_near, [0.1, 0.2, 0.3, 1.5]);
        assert_eq!(raw.direction_cone, [0.0, -1.0, 0.0, 0.25]);
        assert_eq!(raw.cone_inner, [0.5, 0.0, 0.0, 0.0]);
    }
}
