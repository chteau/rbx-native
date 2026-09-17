//! Unit tests for [`super`]: the UV/rect maths behind each `ScaleType` and
//! the nine-quad `Slice` build.

use rbx_assets::AssetRef;

use super::*;

fn rect(x: f32, y: f32, width: f32, height: f32) -> GuiRect {
    GuiRect {
        x,
        y,
        width,
        height,
    }
}

fn painted(scale: GuiImageScale) -> Painted {
    Painted {
        asset: AssetRef::Id(1),
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        repeat: [1.0, 1.0],
        scale,
        rect_offset: [0.0, 0.0],
        rect_size: [0.0, 0.0],
        pixelated: false,
    }
}

#[test]
fn an_unset_image_rect_covers_the_whole_texture() {
    let (uv0, uv1) = sub_rect(&painted(GuiImageScale::Stretch), [200.0, 100.0]);
    assert_eq!((uv0, uv1), ([0.0, 0.0], [1.0, 1.0]));
}

#[test]
fn image_rect_offset_and_size_carve_a_uv_sub_rect() {
    let mut image = painted(GuiImageScale::Stretch);
    image.rect_offset = [10.0, 20.0];
    image.rect_size = [30.0, 40.0];

    let (uv0, uv1) = sub_rect(&image, [100.0, 100.0]);

    assert_eq!(uv0, [0.1, 0.2]);
    assert_eq!(uv1, [0.4, 0.6]);
}

#[test]
fn fit_letterboxes_a_wide_image_with_bars_top_and_bottom() {
    // A 200x100 image (2:1) fit into a 100x100 box: width-limited, so it
    // shrinks to 100x50 and centres vertically.
    let (drawn, uv0, uv1) = fit(
        &rect(0.0, 0.0, 100.0, 100.0),
        &painted(GuiImageScale::Fit),
        [200.0, 100.0],
    );

    assert_eq!(drawn, rect(0.0, 25.0, 100.0, 50.0));
    assert_eq!((uv0, uv1), ([0.0, 0.0], [1.0, 1.0]));
}

#[test]
fn fit_letterboxes_a_tall_image_with_bars_left_and_right() {
    // A 100x200 image (1:2) fit into a 100x100 box: height-limited, so it
    // shrinks to 50x100 and centres horizontally.
    let (drawn, ..) = fit(
        &rect(0.0, 0.0, 100.0, 100.0),
        &painted(GuiImageScale::Fit),
        [100.0, 200.0],
    );

    assert_eq!(drawn, rect(25.0, 0.0, 50.0, 100.0));
}

#[test]
fn fit_never_grows_the_quad_past_the_box() {
    let (drawn, ..) = fit(
        &rect(10.0, 10.0, 40.0, 40.0),
        &painted(GuiImageScale::Fit),
        [10.0, 10.0],
    );

    assert!(drawn.width <= 40.0 && drawn.height <= 40.0);
}

#[test]
fn crop_cuts_the_uvs_of_the_axis_that_overflows() {
    // A square source cropped into a 2:1 box keeps the full width and half
    // the height, centred.
    let (uv0, uv1) = crop(
        &rect(0.0, 0.0, 100.0, 50.0),
        &painted(GuiImageScale::Crop),
        [100.0, 100.0],
    );

    assert_eq!(uv0, [0.0, 0.25]);
    assert_eq!(uv1, [1.0, 0.75]);
}

#[test]
fn crop_of_a_box_matching_the_images_aspect_uses_the_whole_image() {
    let (uv0, uv1) = crop(
        &rect(0.0, 0.0, 100.0, 50.0),
        &painted(GuiImageScale::Crop),
        [200.0, 100.0],
    );

    assert_eq!((uv0, uv1), ([0.0, 0.0], [1.0, 1.0]));
}

#[test]
fn slice_borders_grow_with_slice_scale() {
    let mut vertices = Vec::new();
    let center = GuiPixelRect {
        min: [10.0, 10.0],
        max: [90.0, 90.0],
    };
    slice(
        &rect(0.0, 0.0, 200.0, 150.0),
        &painted(GuiImageScale::Slice {
            center: Some(center),
            scale: 2.0,
        }),
        [100.0, 100.0],
        Some(center),
        2.0,
        &Spin::new(0.0, [100.0, 75.0]),
        &mut vertices,
    );

    // The top-left corner quad is the first one built: 10px source border
    // doubled by `SliceScale` is 20 screen pixels on each side.
    let corner_span: f32 = vertices[..6]
        .iter()
        .map(|v| v.position[0])
        .fold(f32::MIN, f32::max)
        - vertices[..6]
            .iter()
            .map(|v| v.position[0])
            .fold(f32::MAX, f32::min);
    assert_eq!(corner_span, 20.0);
}

#[test]
fn slice_builds_nine_quads_with_a_stretched_centre() {
    let mut vertices = Vec::new();
    let center = GuiPixelRect {
        min: [10.0, 10.0],
        max: [90.0, 90.0],
    };
    slice(
        &rect(0.0, 0.0, 200.0, 150.0),
        &painted(GuiImageScale::Slice {
            center: Some(center),
            scale: 1.0,
        }),
        [100.0, 100.0],
        Some(center),
        1.0,
        &Spin::new(0.0, [100.0, 75.0]),
        &mut vertices,
    );

    // Nine quads, six vertices apiece, none dropped as zero-sized: the box
    // is comfortably bigger than the borders on every side.
    assert_eq!(vertices.len(), 9 * 6);

    // Every corner is 10x10 (the unscaled `SliceCenter` border); the last
    // quad built is the bottom-right corner.
    let bottom_right = &vertices[8 * 6..];
    let width = bottom_right
        .iter()
        .map(|v| v.position[0])
        .fold(f32::MIN, f32::max)
        - bottom_right
            .iter()
            .map(|v| v.position[0])
            .fold(f32::MAX, f32::min);
    assert_eq!(width, 10.0);

    // The centre (the fifth quad, index 4) stretches to fill what the
    // borders leave: 200 - 2*10 = 180 wide, 150 - 2*10 = 130 tall.
    let centre = &vertices[4 * 6..5 * 6];
    let centre_width = centre
        .iter()
        .map(|v| v.position[0])
        .fold(f32::MIN, f32::max)
        - centre
            .iter()
            .map(|v| v.position[0])
            .fold(f32::MAX, f32::min);
    let centre_height = centre
        .iter()
        .map(|v| v.position[1])
        .fold(f32::MIN, f32::max)
        - centre
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MAX, f32::min);
    assert_eq!(centre_width, 180.0);
    assert_eq!(centre_height, 130.0);
}

#[test]
fn slice_falls_back_to_a_single_quad_for_a_zero_sized_texture() {
    let mut vertices = Vec::new();
    slice(
        &rect(0.0, 0.0, 50.0, 50.0),
        &painted(GuiImageScale::Slice {
            center: None,
            scale: 1.0,
        }),
        [0.0, 0.0],
        None,
        1.0,
        &Spin::new(0.0, [25.0, 25.0]),
        &mut vertices,
    );

    assert_eq!(vertices.len(), 6);
}

#[test]
fn opposing_borders_shrink_together_rather_than_overlapping() {
    assert_eq!(shrink_to_fit(60.0, 60.0, 100.0), (50.0, 50.0));
    assert_eq!(shrink_to_fit(10.0, 20.0, 100.0), (10.0, 20.0));
}

#[test]
fn build_dispatches_tile_to_the_plain_repeat_path_ignoring_any_sub_rect() {
    let mut vertices = Vec::new();
    let mut image = painted(GuiImageScale::Tile);
    image.repeat = [3.0, 2.0];
    image.rect_offset = [10.0, 10.0];
    image.rect_size = [10.0, 10.0];

    build(
        &rect(0.0, 0.0, 300.0, 200.0),
        &image,
        [100.0, 100.0],
        &Spin::new(0.0, [150.0, 100.0]),
        &mut vertices,
    );

    let uvs: Vec<[f32; 2]> = vertices.iter().map(|v| v.uv).collect();
    assert!(uvs.contains(&[0.0, 0.0]));
    assert!(uvs.contains(&[3.0, 2.0]));
}
