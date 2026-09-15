//! Decodes the top mip of a DXT1/DXT3/DXT5-compressed DDS file to RGBA8.
//!
//! Roblox ships its default skybox and many particle textures this way inside
//! its native content packages (see [`crate::native`]) — some renamed `.tex`,
//! which is the same format under another extension since detection here goes
//! by the `"DDS "` magic, never by the caller's file name.
//!
//! Only the classic header with a FourCC pixel format is handled; a DX10
//! header or an uncompressed pixel format is rejected with a clear error
//! rather than guessed at.

use image::RgbaImage;

use crate::error::AssetError;

/// Bytes making up the `DDS_HEADER` struct, not counting the 4-byte magic
/// that precedes it or the `DDS_PIXELFORMAT` it embeds at offset 72.
const HEADER_LEN: usize = 124;
// `DDS_PIXELFORMAT` starts at header offset 72 (after `dwReserved1`); its
// `dwFourCC` follows `dwSize` and `dwFlags`, two `u32`s in.
const PIXEL_FORMAT_FOURCC_OFFSET: usize = 72 + 8;
const DATA_OFFSET: usize = 4 + HEADER_LEN;

struct Header {
    width: u32,
    height: u32,
    four_cc: [u8; 4],
}

fn parse_header(data: &[u8]) -> Result<Header, AssetError> {
    if data.len() < DATA_OFFSET || &data[0..4] != b"DDS " {
        return Err(AssetError::ImageDecode(
            "not a DDS file (missing \"DDS \" magic)".to_string(),
        ));
    }
    let header = &data[4..DATA_OFFSET];
    let size = u32::from_le_bytes(header[0..4].try_into().unwrap());
    if size != HEADER_LEN as u32 {
        return Err(AssetError::ImageDecode(format!(
            "unexpected DDS header size {size}, expected {HEADER_LEN}"
        )));
    }
    let height = u32::from_le_bytes(header[8..12].try_into().unwrap());
    let width = u32::from_le_bytes(header[12..16].try_into().unwrap());
    let four_cc: [u8; 4] = header[PIXEL_FORMAT_FOURCC_OFFSET..PIXEL_FORMAT_FOURCC_OFFSET + 4]
        .try_into()
        .unwrap();
    Ok(Header {
        width,
        height,
        four_cc,
    })
}

/// Decodes a DDS asset's top mip to RGBA8, using only its own dimensions.
///
/// Anything beyond the first mip is ignored: the sniffed [`crate::AssetKind`]
/// exists to pick a decoder, not to stream a whole mip chain into a texture
/// the viewer never asked for.
pub(crate) fn decode(data: &[u8]) -> Result<RgbaImage, AssetError> {
    let header = parse_header(data)?;
    let pixels = data.get(DATA_OFFSET..).unwrap_or(&[]);
    match &header.four_cc {
        b"DXT1" => decode_blocks(header.width, header.height, pixels, 8, decode_bc1_block),
        b"DXT3" => decode_blocks(header.width, header.height, pixels, 16, decode_bc2_block),
        b"DXT5" => decode_blocks(header.width, header.height, pixels, 16, decode_bc3_block),
        other => Err(AssetError::ImageDecode(format!(
            "DDS pixel format {:?} is not supported, only DXT1/DXT3/DXT5",
            String::from_utf8_lossy(other)
        ))),
    }
}

/// Walks the block grid, decoding each 4x4 block and copying it into the
/// image — clipped on the right/bottom edge for dimensions not a multiple of 4,
/// which a compressed block always pads up to.
fn decode_blocks(
    width: u32,
    height: u32,
    data: &[u8],
    block_len: usize,
    decode_block: fn(&[u8]) -> [[u8; 4]; 16],
) -> Result<RgbaImage, AssetError> {
    if width == 0 || height == 0 {
        return Err(AssetError::ImageDecode(
            "DDS image has a zero dimension".to_string(),
        ));
    }
    let blocks_wide = width.div_ceil(4) as usize;
    let blocks_high = height.div_ceil(4) as usize;
    let needed = blocks_wide * blocks_high * block_len;
    if data.len() < needed {
        return Err(AssetError::ImageDecode(format!(
            "DDS top mip is truncated: need {needed} bytes, have {}",
            data.len()
        )));
    }

    let mut image = RgbaImage::new(width, height);
    for by in 0..blocks_high {
        for bx in 0..blocks_wide {
            let offset = (by * blocks_wide + bx) * block_len;
            let block = decode_block(&data[offset..offset + block_len]);
            for (i, pixel) in block.iter().enumerate() {
                let x = bx as u32 * 4 + (i % 4) as u32;
                let y = by as u32 * 4 + (i / 4) as u32;
                if x < width && y < height {
                    image.put_pixel(x, y, image::Rgba(*pixel));
                }
            }
        }
    }
    Ok(image)
}

fn unpack_565(value: u16) -> (u8, u8, u8) {
    let r5 = ((value >> 11) & 0x1F) as u8;
    let g6 = ((value >> 5) & 0x3F) as u8;
    let b5 = (value & 0x1F) as u8;
    // Bit-replication expansion (e.g. `r << 3 | r >> 2`) is what every real
    // decoder uses to fill the low bits, rather than a lossier `* 255 / 31`.
    (
        (r5 << 3) | (r5 >> 2),
        (g6 << 2) | (g6 >> 4),
        (b5 << 3) | (b5 >> 2),
    )
}

fn lerp_third(a: u8, b: u8, weight_a: u32, weight_b: u32) -> u8 {
    ((a as u32 * weight_a + b as u32 * weight_b) / 3) as u8
}

/// The 8-byte color half shared by BC1/BC2/BC3: two RGB565 endpoints and a
/// 2-bit index per pixel. `punch_through_alpha` enables BC1's one-bit-alpha
/// mode (`color0 <= color1` as raw `u16`s); BC2/BC3 always decode the four
/// opaque colors and get their alpha from a separate block instead.
fn decode_color_block(block: &[u8], punch_through_alpha: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (r0, g0, b0) = unpack_565(c0);
    let (r1, g1, b1) = unpack_565(c1);
    let is_punch_through = punch_through_alpha && c0 <= c1;

    let palette: [[u8; 4]; 4] = if is_punch_through {
        [
            [r0, g0, b0, 255],
            [r1, g1, b1, 255],
            [
                ((r0 as u16 + r1 as u16) / 2) as u8,
                ((g0 as u16 + g1 as u16) / 2) as u8,
                ((b0 as u16 + b1 as u16) / 2) as u8,
                255,
            ],
            [0, 0, 0, 0],
        ]
    } else {
        [
            [r0, g0, b0, 255],
            [r1, g1, b1, 255],
            [
                lerp_third(r0, r1, 2, 1),
                lerp_third(g0, g1, 2, 1),
                lerp_third(b0, b1, 2, 1),
                255,
            ],
            [
                lerp_third(r0, r1, 1, 2),
                lerp_third(g0, g1, 1, 2),
                lerp_third(b0, b1, 1, 2),
                255,
            ],
        ]
    };

    let indices = u32::from_le_bytes(block[4..8].try_into().unwrap());
    let mut pixels = [[0u8; 4]; 16];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let index = ((indices >> (i * 2)) & 0b11) as usize;
        *pixel = palette[index];
    }
    pixels
}

fn decode_bc1_block(block: &[u8]) -> [[u8; 4]; 16] {
    decode_color_block(block, true)
}

fn decode_bc2_block(block: &[u8]) -> [[u8; 4]; 16] {
    let mut pixels = decode_color_block(&block[8..16], false);
    apply_explicit_alpha(&mut pixels, &block[0..8]);
    pixels
}

fn decode_bc3_block(block: &[u8]) -> [[u8; 4]; 16] {
    let mut pixels = decode_color_block(&block[8..16], false);
    apply_interpolated_alpha(&mut pixels, &block[0..8]);
    pixels
}

/// BC2's alpha half: a plain 4-bit value per pixel, two pixels per byte.
fn apply_explicit_alpha(pixels: &mut [[u8; 4]; 16], alpha_block: &[u8]) {
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let byte = alpha_block[i / 2];
        let nibble = if i % 2 == 0 { byte & 0x0F } else { byte >> 4 };
        // 15 * 17 == 255, so this expansion hits both ends of the range exactly.
        pixel[3] = nibble * 17;
    }
}

/// BC3's alpha half: two 8-bit endpoints plus a 3-bit index per pixel, either
/// interpolating between them (`a0 > a1`) or reserving two of the eight slots
/// for flat 0 and 255 (`a0 <= a1`) — mirroring BC1's color/alpha split.
fn apply_interpolated_alpha(pixels: &mut [[u8; 4]; 16], alpha_block: &[u8]) {
    let a0 = alpha_block[0];
    let a1 = alpha_block[1];
    let (a0, a1) = (a0 as u32, a1 as u32);
    let palette: [u8; 8] = if a0 > a1 {
        [
            a0 as u8,
            a1 as u8,
            ((6 * a0 + a1) / 7) as u8,
            ((5 * a0 + 2 * a1) / 7) as u8,
            ((4 * a0 + 3 * a1) / 7) as u8,
            ((3 * a0 + 4 * a1) / 7) as u8,
            ((2 * a0 + 5 * a1) / 7) as u8,
            ((a0 + 6 * a1) / 7) as u8,
        ]
    } else {
        [
            a0 as u8,
            a1 as u8,
            ((4 * a0 + a1) / 5) as u8,
            ((3 * a0 + 2 * a1) / 5) as u8,
            ((2 * a0 + 3 * a1) / 5) as u8,
            ((a0 + 4 * a1) / 5) as u8,
            0,
            255,
        ]
    };

    // The six index bytes are one little-endian 48-bit integer, 3 bits/pixel.
    let mut index_bits: u64 = 0;
    for (i, &byte) in alpha_block[2..8].iter().enumerate() {
        index_bits |= (byte as u64) << (i * 8);
    }
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let index = ((index_bits >> (i * 3)) & 0b111) as usize;
        pixel[3] = palette[index];
    }
}

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests;
