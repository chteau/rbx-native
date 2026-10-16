use super::{from_colorref, from_masked_pixel, from_unit};

#[test]
fn unit_channels_map_to_8_bit() {
    assert_eq!(from_unit(0.0, 0.5, 1.0), [0, 128, 255]);
}

#[test]
fn unit_channels_clamp_outside_range() {
    assert_eq!(from_unit(-0.3, 1.7, -1.0), [0, 255, 0]);
}

#[test]
fn masked_pixel_rgb_order() {
    assert_eq!(
        from_masked_pixel(0x123456, 0xff0000, 0x00ff00, 0x0000ff),
        Some([0x12, 0x34, 0x56])
    );
}

#[test]
fn masked_pixel_bgr_order() {
    assert_eq!(
        from_masked_pixel(0x123456, 0x0000ff, 0x00ff00, 0xff0000),
        Some([0x56, 0x34, 0x12])
    );
}

#[test]
fn masked_pixel_565_scales_to_full_range() {
    assert_eq!(
        from_masked_pixel(0xffff, 0xf800, 0x07e0, 0x001f),
        Some([255, 255, 255])
    );
    assert_eq!(
        from_masked_pixel(0xf800, 0xf800, 0x07e0, 0x001f),
        Some([255, 0, 0])
    );
    assert_eq!(
        from_masked_pixel(0x001f, 0xf800, 0x07e0, 0x001f),
        Some([0, 0, 255])
    );
}

#[test]
fn masked_pixel_zero_mask_is_none() {
    assert_eq!(from_masked_pixel(0x123456, 0, 0x00ff00, 0x0000ff), None);
    assert_eq!(from_masked_pixel(0x123456, 0xff0000, 0, 0x0000ff), None);
    assert_eq!(from_masked_pixel(0x123456, 0xff0000, 0x00ff00, 0), None);
}

#[test]
fn masked_pixel_10_bit_keeps_top_8_bits() {
    assert_eq!(
        from_masked_pixel(0x3ff00000, 0x3ff00000, 0x000ffc00, 0x000003ff),
        Some([255, 0, 0])
    );
    assert_eq!(
        from_masked_pixel(0x000ffc00, 0x3ff00000, 0x000ffc00, 0x000003ff),
        Some([0, 255, 0])
    );
}

#[test]
fn colorref_is_bgr() {
    assert_eq!(from_colorref(0x00563412), [0x12, 0x34, 0x56]);
}
