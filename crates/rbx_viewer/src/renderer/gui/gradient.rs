//! Every `UIGradient` a painter draws, baked into one row each of a small
//! texture the fragment shader reads its ramp from (bind group 2).
//!
//! A row is the ramp sampled at [`WIDTH`] evenly spaced points, colour in
//! sRGB — the space Roblox interpolates a `ColorSequence` in — and
//! `1 - Transparency` in alpha; the texture is an `*Srgb` format so the GPU
//! linearizes the colour on the way in, matching the linear vertex colour it
//! is multiplied into. Two elements with the same ramp share a row, since the
//! ramp is baked from the sequences alone: geometry travels per vertex.

use std::collections::HashMap;

use super::super::texture;
use crate::scene::{eval_color, eval_number, GuiGradient};

/// Texels per row: plenty for the "at most 6 colour stops" the docs advise.
pub(super) const WIDTH: u32 = 256;
const CHANNELS: usize = 4;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The ramps one quad build handed out rows for, in row order.
#[derive(Default)]
pub(super) struct Rows {
    baked: Vec<Vec<u8>>,
    index: HashMap<Vec<u8>, usize>,
}

impl Rows {
    /// The row `gradient`'s ramp lives on, baking it the first time.
    pub(super) fn row(&mut self, gradient: &GuiGradient) -> usize {
        let ramp = bake(gradient);
        if let Some(&row) = self.index.get(&ramp) {
            return row;
        }
        let row = self.baked.len();
        self.baked.push(ramp.clone());
        self.index.insert(ramp, row);
        row
    }

    pub(super) fn len(&self) -> usize {
        self.baked.len()
    }
}

/// One row of RGBA8 texels: the ramp at `t = i / (WIDTH - 1)`, so the first
/// and last texel are exactly the sequence's ends.
pub(super) fn bake(gradient: &GuiGradient) -> Vec<u8> {
    let mut ramp = Vec::with_capacity(WIDTH as usize * CHANNELS);
    for texel in 0..WIDTH {
        let t = texel as f32 / (WIDTH - 1) as f32;
        let [r, g, b] = eval_color(&gradient.color, t).map(linear_to_srgb);
        let alpha = 1.0 - eval_number(&gradient.transparency, t).clamp(0.0, 1.0);
        ramp.extend([r, g, b, alpha].map(byte));
    }
    ramp
}

/// The inverse of `scene::srgb_to_linear`: `eval_color` linearizes at the
/// end, and the texture wants the sRGB encoding back to store 8 bits of it
/// without banding in the darks.
fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The GPU side: the rows uploaded as one texture and bound with a sampler,
/// rebuilt whenever a build hands out a different set of rows.
pub(super) struct Table {
    /// The plain image layout (`texture::layout`): a texture plus a sampler
    /// is all a ramp needs, so bind group 2 is shaped like bind group 1.
    pub(super) layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
}

impl Table {
    pub(super) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = texture::layout(device);
        // Clamped rather than repeating: tiling is done on `t` in the shader,
        // per `GradientTileMode`, before the ramp is ever sampled.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rbxview gui gradient"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = bind(device, queue, &layout, &sampler, &Rows::default());
        Table {
            layout,
            sampler,
            bind_group,
        }
    }

    /// Replaces the bound texture with `rows`. Always uploads, even for the
    /// same rows as last time: a build is a resize or a rebuild, and the
    /// texture is a few kilobytes.
    pub(super) fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rows: &Rows) {
        self.bind_group = bind(device, queue, &self.layout, &self.sampler, rows);
    }

    pub(super) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// `rows` as a `WIDTH` × rows texture — one blank row where there are none,
/// since a bind group cannot hold an empty texture and a vertex without a
/// gradient never samples it anyway.
fn bind(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    rows: &Rows,
) -> wgpu::BindGroup {
    let height = rows.len().max(1) as u32;
    let size = wgpu::Extent3d {
        width: WIDTH,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rbxview gui gradient"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut texels: Vec<u8> = rows.baked.concat();
    texels.resize(WIDTH as usize * CHANNELS * height as usize, 0);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(WIDTH * CHANNELS as u32),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rbxview gui gradient"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Color3Data, ColorSequenceKeypoint, NumberSequenceKeypoint};

    use super::*;
    use crate::scene::{GuiGradientKind, GuiTile};

    fn ramp(colors: &[(f32, [f32; 3])], transparency: &[(f32, f32)]) -> GuiGradient {
        GuiGradient {
            color: rbx_dom::ColorSequence {
                keypoints: colors
                    .iter()
                    .map(|&(time, [r, g, b])| ColorSequenceKeypoint {
                        time,
                        color: Color3Data { r, g, b },
                        envelope: 0.0,
                    })
                    .collect(),
            },
            transparency: rbx_dom::NumberSequence {
                keypoints: transparency
                    .iter()
                    .map(|&(time, value)| NumberSequenceKeypoint {
                        time,
                        value,
                        envelope: 0.0,
                    })
                    .collect(),
            },
            origin: [0.0, 0.0],
            axis: [1.0, 0.0],
            kind: GuiGradientKind::Linear,
            tile: GuiTile::Clamp,
        }
    }

    fn texel(ramp: &[u8], index: usize) -> [u8; 4] {
        ramp[index * CHANNELS..index * CHANNELS + CHANNELS]
            .try_into()
            .unwrap()
    }

    #[test]
    fn a_ramp_is_a_row_of_texels_running_from_the_first_keypoint_to_the_last() {
        let baked = bake(&ramp(
            &[(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])],
            &[(0.0, 0.0)],
        ));

        assert_eq!(baked.len(), WIDTH as usize * CHANNELS);
        assert_eq!(texel(&baked, 0), [255, 0, 0, 255]);
        assert_eq!(texel(&baked, WIDTH as usize - 1), [0, 0, 255, 255]);
    }

    // Roblox lerps a `ColorSequence` in its own sRGB values, so the middle
    // of red → blue is (0.5, 0, 0.5) *in sRGB* — 128, not the 188 that a
    // lerp in linear light re-encoded would give.
    #[test]
    fn keypoints_are_interpolated_in_srgb() {
        let baked = bake(&ramp(
            &[(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])],
            &[(0.0, 0.0)],
        ));

        let [r, g, b, _] = texel(&baked, 127);
        assert!((127..=129).contains(&r), "{r}");
        assert_eq!(g, 0);
        assert!((126..=128).contains(&b), "{b}");
    }

    #[test]
    fn transparency_becomes_alpha() {
        let baked = bake(&ramp(&[(0.0, [1.0; 3])], &[(0.0, 0.0), (1.0, 0.75)]));

        assert_eq!(texel(&baked, 0)[3], 255);
        assert_eq!(texel(&baked, WIDTH as usize - 1)[3], 64);
    }

    // Two keypoints at one time are a hard step, as they are in Studio.
    #[test]
    fn two_keypoints_at_the_same_time_step() {
        let baked = bake(&ramp(
            &[
                (0.0, [1.0, 0.0, 0.0]),
                (0.5, [1.0, 0.0, 0.0]),
                (0.5, [0.0, 0.0, 1.0]),
                (1.0, [0.0, 0.0, 1.0]),
            ],
            &[(0.0, 0.0)],
        ));

        assert_eq!(texel(&baked, 126), [255, 0, 0, 255]);
        assert_eq!(texel(&baked, 129), [0, 0, 255, 255]);
    }

    #[test]
    fn identical_ramps_share_a_row_and_different_ones_do_not() {
        let mut rows = Rows::default();
        let red = ramp(&[(0.0, [1.0, 0.0, 0.0])], &[(0.0, 0.0)]);
        let faded = ramp(&[(0.0, [1.0, 0.0, 0.0])], &[(0.0, 0.5)]);

        assert_eq!(rows.row(&red), 0);
        assert_eq!(rows.row(&faded), 1);
        assert_eq!(rows.row(&red), 0);
        assert_eq!(rows.len(), 2);
    }
}
