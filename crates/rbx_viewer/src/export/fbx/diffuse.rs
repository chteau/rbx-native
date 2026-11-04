//! The colour map an FBX material points at, made to stand on its own.
//!
//! glTF multiplies its colour texture by the material's colour, and our
//! viewport does the same with a part's `Color`, so the material packs come
//! out untinted. FBX readers don't: Blender, Maya and Studio's importer all
//! take a texture on `DiffuseColor` in place of the colour. So the tint is
//! baked into the image here, in linear light as the viewport mixes it.
//!
//! The same readers also take an RGBA image's alpha as opacity. That is
//! right for a decal or a SurfaceAppearance cut-out, which is why the mesh
//! blends at all, but a material pack's alpha is a mask of its own (Glass's
//! is under 0.2) and a part's `Transparency` already goes in the material.
//! So alpha stays only on an opaque part that blends because of its image.

use std::io::Cursor;

use crate::export::{ExportMesh, Finish};
use crate::scene::{linear_to_srgb, srgb_to_linear};

/// What makes two meshes' colour maps the same file.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Key {
    index: usize,
    tint: [u32; 3],
    alpha: bool,
}

impl Key {
    pub(super) fn index(self) -> usize {
        self.index
    }

    pub(super) fn of(mesh: &ExportMesh, index: usize) -> Self {
        let [r, g, b, a] = mesh.color;
        let alpha = mesh.blend && a >= 1.0 && matches!(mesh.finish, Finish::Plain | Finish::Neon);
        Self {
            index,
            tint: [r, g, b].map(f32::to_bits),
            alpha,
        }
    }
}

/// `png` tinted and with its alpha kept or dropped as `key` says. An image
/// that won't decode goes through as it is.
pub(super) fn bake(png: &[u8], key: Key) -> Vec<u8> {
    let tint = key.tint.map(f32::from_bits);
    if tint == [1.0; 3] && key.alpha {
        return png.to_vec();
    }
    decode(png)
        .and_then(|(width, height, rgba)| {
            encode(width, height, &tinted(&rgba, tint, key.alpha), key.alpha)
        })
        .unwrap_or_else(|| png.to_vec())
}

/// RGBA texels times `tint`, three or four bytes each out.
fn tinted(rgba: &[u8], tint: [f32; 3], alpha: bool) -> Vec<u8> {
    let linear: Vec<f32> = (0..=255u8)
        .map(|v| srgb_to_linear(f32::from(v) / 255.0))
        .collect();
    let channels = if alpha { 4 } else { 3 };
    let mut out = Vec::with_capacity(rgba.len() / 4 * channels);
    for texel in rgba.as_chunks::<4>().0 {
        for (c, &factor) in tint.iter().enumerate() {
            let value = linear_to_srgb(linear[usize::from(texel[c])] * factor.clamp(0.0, 1.0));
            out.push((value * 255.0).round().clamp(0.0, 255.0) as u8);
        }
        if alpha {
            out.push(texel[3]);
        }
    }
    out
}

/// A linear `color` as the sRGB texel of its swatch.
pub(super) fn swatch_texel(color: [f32; 4]) -> [u8; 3] {
    [color[0], color[1], color[2]]
        .map(|c| (linear_to_srgb(c.clamp(0.0, 1.0)) * 255.0).round() as u8)
}

/// A 1x1 PNG of `texel`, the whole colour map of a plain surface.
pub(super) fn swatch(texel: [u8; 3]) -> Vec<u8> {
    encode(1, 1, &texel, false).unwrap_or_default()
}

fn decode(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let bytes = &buffer[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|t| [t[0], t[1], t[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|t| [t[0], t[0], t[0], t[1]])
            .collect(),
        png::ColorType::Grayscale => bytes.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((info.width, info.height, rgba))
}

fn encode(width: u32, height: u32, texels: &[u8], alpha: bool) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(if alpha {
        png::ColorType::Rgba
    } else {
        png::ColorType::Rgb
    });
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(texels).ok()?;
    writer.finish().ok()?;
    Some(bytes)
}
