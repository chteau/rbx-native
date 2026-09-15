//! Unit tests for the DDS header parser and BC1/BC2/BC3 block decoders.

use super::*;

fn dds_header(width: u32, height: u32, four_cc: &[u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(DATA_OFFSET);
    bytes.extend_from_slice(b"DDS ");
    bytes.extend_from_slice(&(HEADER_LEN as u32).to_le_bytes()); // dwSize
    bytes.extend_from_slice(&0u32.to_le_bytes()); // dwFlags
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // dwPitchOrLinearSize
    bytes.extend_from_slice(&0u32.to_le_bytes()); // dwDepth
    bytes.extend_from_slice(&1u32.to_le_bytes()); // dwMipMapCount
    bytes.extend_from_slice(&[0u8; 44]); // dwReserved1
    bytes.extend_from_slice(&32u32.to_le_bytes()); // DDS_PIXELFORMAT.dwSize
    bytes.extend_from_slice(&0x04u32.to_le_bytes()); // DDPF_FOURCC
    bytes.extend_from_slice(four_cc);
    bytes.extend_from_slice(&[0u8; 20]); // bit counts + masks
    bytes.extend_from_slice(&[0u8; 20]); // dwCaps..dwReserved2
    assert_eq!(bytes.len(), DATA_OFFSET);
    bytes
}

/// RGB565 for pure red, packed little-endian, as both block endpoints so
/// the whole 4x4 block decodes to one flat opaque color.
fn solid_color_bc1_block(color565: u16) -> [u8; 8] {
    let mut block = [0u8; 8];
    block[0..2].copy_from_slice(&color565.to_le_bytes());
    block[2..4].copy_from_slice(&color565.to_le_bytes());
    // Indices left at 0: every pixel picks palette entry 0 (== color0).
    block
}

#[test]
fn header_round_trips_width_height_and_fourcc() {
    let bytes = dds_header(512, 256, b"DXT5");
    let header = parse_header(&bytes).unwrap();
    assert_eq!(header.width, 512);
    assert_eq!(header.height, 256);
    assert_eq!(&header.four_cc, b"DXT5");
}

#[test]
fn rejects_missing_magic() {
    let err = parse_header(b"not a dds file at all, padded to be long enough...........");
    assert!(err.is_err());
}

#[test]
fn rejects_unsupported_fourcc() {
    let mut bytes = dds_header(4, 4, b"DXT1");
    bytes.extend_from_slice(&solid_color_bc1_block(0xF800));
    bytes[PIXEL_FORMAT_FOURCC_OFFSET + 4..PIXEL_FORMAT_FOURCC_OFFSET + 4 + 4]
        .copy_from_slice(b"BC7\0");
    let err = decode(&bytes);
    assert!(err.is_err());
}

#[test]
fn decodes_a_solid_red_dxt1_block() {
    let mut bytes = dds_header(4, 4, b"DXT1");
    // 0xF800 is pure red in RGB565 (R=31, G=0, B=0).
    bytes.extend_from_slice(&solid_color_bc1_block(0xF800));
    let image = decode(&bytes).unwrap();
    assert_eq!(image.dimensions(), (4, 4));
    for pixel in image.pixels() {
        assert_eq!(*pixel, image::Rgba([255, 0, 0, 255]));
    }
}

#[test]
fn dxt1_punch_through_alpha_index_is_fully_transparent() {
    // color0 (0x0000, black) <= color1 (0xFFFF, white) selects the
    // one-bit-alpha mode; every pixel here picks index 3, which that mode
    // defines as transparent black instead of the color2 average.
    let mut block = [0u8; 8];
    block[2..4].copy_from_slice(&0xFFFFu16.to_le_bytes());
    block[4..8].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    let mut bytes = dds_header(4, 4, b"DXT1");
    bytes.extend_from_slice(&block);
    let image = decode(&bytes).unwrap();
    for pixel in image.pixels() {
        assert_eq!(*pixel, image::Rgba([0, 0, 0, 0]));
    }
}

#[test]
fn dxt5_alpha_ramp_interpolates_between_the_two_endpoints() {
    // a0=255 > a1=0 selects 8-alpha interpolation; indices 0..=7 walk the
    // ramp from opaque to transparent, one per pixel of the first row.
    let mut alpha_block = [0u8; 8];
    alpha_block[0] = 255;
    alpha_block[1] = 0;
    let mut index_bits: u64 = 0;
    for i in 0..16u64 {
        index_bits |= (i % 8) << (i * 3);
    }
    for (i, byte) in alpha_block[2..8].iter_mut().enumerate() {
        *byte = ((index_bits >> (i * 8)) & 0xFF) as u8;
    }
    let mut block = alpha_block.to_vec();
    block.extend_from_slice(&solid_color_bc1_block(0xFFFF)); // white, irrelevant here

    let mut bytes = dds_header(4, 4, b"DXT5");
    bytes.extend_from_slice(&block);
    let image = decode(&bytes).unwrap();

    // A single 4x4 block has no eighth column; walk its 16 pixels in
    // raster order instead, which is where indices 0..=7 actually land.
    let alphas: Vec<u8> = (0..8).map(|i| image.get_pixel(i % 4, i / 4).0[3]).collect();
    assert_eq!(alphas[0], 255);
    assert_eq!(alphas[1], 0);
    // Interpolated steps must fall strictly between the two endpoints and
    // strictly decrease as the index climbs from 2 to 7.
    for pair in alphas[2..8].windows(2) {
        assert!(pair[0] > pair[1], "{alphas:?}");
    }
    assert!(alphas[2] < 255 && alphas[7] > 0, "{alphas:?}");
}

#[test]
fn bc2_explicit_alpha_expands_nibbles_to_the_full_byte_range() {
    let mut alpha_block = [0u8; 8];
    alpha_block[0] = 0x0F; // low nibble (pixel0) = 0xF -> 255, high (pixel1) = 0x0 -> 0
    let mut block = alpha_block.to_vec();
    block.extend_from_slice(&solid_color_bc1_block(0xFFFF));

    let mut bytes = dds_header(4, 4, b"DXT3");
    bytes.extend_from_slice(&block);
    let image = decode(&bytes).unwrap();
    assert_eq!(image.get_pixel(0, 0).0[3], 255);
    assert_eq!(image.get_pixel(1, 0).0[3], 0);
}

#[test]
fn dimensions_not_a_multiple_of_four_are_clipped_to_the_requested_size() {
    let mut bytes = dds_header(3, 3, b"DXT1");
    bytes.extend_from_slice(&solid_color_bc1_block(0xF800));
    let image = decode(&bytes).unwrap();
    assert_eq!(image.dimensions(), (3, 3));
}

#[test]
fn truncated_top_mip_is_reported_rather_than_panicking() {
    let mut bytes = dds_header(8, 8, b"DXT1");
    bytes.extend_from_slice(&solid_color_bc1_block(0xF800)); // only 1 of 4 blocks
    assert!(decode(&bytes).is_err());
}
