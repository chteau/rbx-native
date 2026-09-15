//! Cosine-weighted sky irradiance, one value per cube map axis.
//!
//! `EnvironmentDiffuseScale` lights a surface with the *irradiance* of the sky
//! above it, not with the single cube map texel its normal points at: an
//! upward face in Studio picks up the bright white band round the horizon as
//! well as the deep blue zenith, which is why a grey baseplate comes out
//! blue-grey rather than blue. Integrating that on the CPU once, for the six
//! cardinal normals, lets the shader rebuild it with three multiplies.

use crate::assets::Image;
use crate::scene::srgb_to_linear;

const CHANNELS: usize = 4;
pub(super) const FACES: usize = 6;

/// The unit axis each cube map face looks down, in the layer order the cube is
/// built in (+X, -X, +Y, -Y, +Z, -Z).
const AXES: [[f32; 3]; FACES] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// Irradiance for the six cardinal normals, normalized so a sky of uniform
/// radiance `L` answers `L` in every direction.
///
/// `faces` are the six cube map faces, smallest useful mip first choice of the
/// caller: the integral converges long before full resolution, and every extra
/// level costs four times the texels for no visible difference.
pub(super) fn axis_irradiance(faces: &[Image]) -> [[f32; 4]; FACES] {
    let mut sums = [[0.0f32; 3]; FACES];
    let mut weights = [0.0f32; FACES];

    for (face, image) in faces.iter().enumerate().take(FACES) {
        for (direction, solid_angle, color) in texels(face, image) {
            for ((sum, weight), unit) in sums.iter_mut().zip(weights.iter_mut()).zip(AXES) {
                let cosine =
                    direction[0] * unit[0] + direction[1] * unit[1] + direction[2] * unit[2];
                if cosine <= 0.0 {
                    continue;
                }
                let share = solid_angle * cosine;
                *weight += share;
                for (channel, value) in sum.iter_mut().zip(color) {
                    *channel += share * value;
                }
            }
        }
    }

    let mut irradiance = [[0.0f32; 4]; FACES];
    for ((out, sum), weight) in irradiance.iter_mut().zip(sums).zip(weights) {
        let scale = if weight > 0.0 { 1.0 / weight } else { 0.0 };
        *out = [sum[0] * scale, sum[1] * scale, sum[2] * scale, 0.0];
    }
    irradiance
}

/// Every texel of one face as (direction, solid angle, linear colour).
///
/// The face parametrization is the GL/D3D cube map convention wgpu inherits,
/// which is the same one `envmap`'s quarter turns bring the sky panels into;
/// the solid angle falls off as `(1 + u^2 + v^2)^-3/2` because the cube's
/// corners sit further from the sphere than its face centres do.
fn texels(face: usize, image: &Image) -> impl Iterator<Item = ([f32; 3], f32, [f32; 3])> + '_ {
    let (width, height) = (image.width.max(1), image.height.max(1));

    (0..height).flat_map(move |y| {
        (0..width).map(move |x| {
            let u = (x as f32 + 0.5) / width as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / height as f32 * 2.0 - 1.0;
            let raw = match face {
                0 => [1.0, -v, -u],
                1 => [-1.0, -v, u],
                2 => [u, 1.0, v],
                3 => [u, -1.0, -v],
                4 => [u, -v, 1.0],
                _ => [-u, -v, -1.0],
            };
            let length = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
            let direction = raw.map(|component| component / length);
            let solid_angle = 1.0 / length.powi(3);

            let offset = (y as usize * width as usize + x as usize) * CHANNELS;
            let color = [0, 1, 2]
                .map(|channel| srgb_to_linear(f32::from(image.pixels[offset + channel]) / 255.0));

            (direction, solid_angle, color)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(value: u8, size: u32) -> Image {
        Image {
            width: size,
            height: size,
            pixels: vec![value; (size * size) as usize * CHANNELS],
        }
    }

    #[test]
    fn a_uniform_sky_irradiates_every_normal_with_its_own_radiance() {
        let faces: Vec<Image> = (0..FACES).map(|_| flat(u8::MAX, 8)).collect();

        let irradiance = axis_irradiance(&faces);

        for axis in irradiance {
            assert!((axis[0] - 1.0).abs() < 0.01, "{axis:?}");
        }
    }

    #[test]
    fn a_sky_bright_only_overhead_lights_an_upward_normal_most() {
        let mut faces: Vec<Image> = (0..FACES).map(|_| flat(0, 4)).collect();
        faces[2] = flat(u8::MAX, 4);

        let irradiance = axis_irradiance(&faces);

        // +Y sees the lit face head on, -Y not at all, and the four sides catch
        // it at a glancing angle.
        assert!(irradiance[2][0] > irradiance[0][0]);
        assert!(irradiance[0][0] > irradiance[3][0]);
        assert_eq!(irradiance[3][0], 0.0);
    }

    // The half of the sphere behind a normal must not light it, however bright
    // it is: that is what keeps a shaded face shaded. The face a normal points
    // straight at is 55 % of the cosine-weighted hemisphere and the four it
    // grazes the rest, so one lit face alone lands near half, not at one.
    #[test]
    fn a_normal_never_picks_up_the_face_behind_it() {
        let mut faces: Vec<Image> = (0..FACES).map(|_| flat(0, 4)).collect();
        faces[3] = flat(u8::MAX, 4);

        let irradiance = axis_irradiance(&faces);

        assert_eq!(irradiance[2][0], 0.0);
        assert!(
            irradiance[3][0] > 0.5 && irradiance[3][0] < 0.6,
            "{:?}",
            irradiance[3]
        );
    }
}
