//! Unit tests for [`super`]: scissor conversion, border bands and run merging;
//! [`modifiers`] for what a `UICorner`/`UIStroke`/`UIGradient` puts on a vertex.

use super::*;
use crate::scene::{GuiElement, GuiImageScale, Painted};

fn rect(x: f32, y: f32, width: f32, height: f32) -> GuiRect {
    GuiRect {
        x,
        y,
        width,
        height,
    }
}

fn element(rect: GuiRect, clip: Option<GuiRect>) -> GuiElement {
    GuiElement {
        rect,
        clip,
        rotation: 0.0,
        background: [1.0, 0.0, 0.0],
        background_alpha: 1.0,
        border: None,
        border_inset: 0.0,
        z_index: 1,
        image: None,
        corner_radii: [0.0; 4],
        stroke: None,
        gradient: None,
        text: None,
        viewport: None,
    }
}

const VIEWPORT: (u32, u32) = (800, 600);

/// [`super::build`] for elements with no text, which need no typesetter of
/// their own. Shadows the glob import above.
fn build(
    elements: &[GuiElement],
    textures: &HashMap<AssetRef, Slot>,
    target: (u32, u32),
) -> (Vec<VertexRaw>, Vec<Run>, Rows) {
    super::build(elements, textures, target, &mut Typesetter::new())
}

#[test]
fn a_clip_rounds_outwards_so_no_seam_shows_along_its_own_edge() {
    let scissor = scissor(&rect(10.4, 20.6, 100.2, 50.1), VIEWPORT).unwrap();

    assert_eq!(
        scissor,
        Scissor {
            x: 10,
            y: 20,
            width: 101,
            height: 51,
        }
    );
}

#[test]
fn a_clip_running_off_screen_is_clamped_to_the_target() {
    let scissor = scissor(&rect(-50.0, -50.0, 10_000.0, 10_000.0), VIEWPORT).unwrap();

    assert_eq!(
        scissor,
        Scissor {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        }
    );
}

// `wgpu` rejects a zero-sized scissor, and a fully clipped element has nothing
// to show through one anyway.
#[test]
fn a_clip_with_nothing_left_on_screen_has_no_scissor_at_all() {
    assert!(scissor(&rect(900.0, 0.0, 100.0, 100.0), VIEWPORT).is_none());
    assert!(scissor(&rect(0.0, 0.0, 0.0, 0.0), VIEWPORT).is_none());
    assert!(scissor(&rect(-200.0, 0.0, 100.0, 100.0), VIEWPORT).is_none());
}

#[test]
fn an_element_clipped_away_entirely_contributes_no_vertices() {
    let elements = [element(
        rect(0.0, 0.0, 100.0, 100.0),
        Some(rect(900.0, 900.0, 10.0, 10.0)),
    )];

    let (vertices, runs, _) = build(&elements, &HashMap::new(), VIEWPORT);

    assert!(vertices.is_empty());
    assert!(runs.is_empty());
}

#[test]
fn a_border_covers_each_corner_exactly_once() {
    let bands = outline(&rect(100.0, 100.0, 50.0, 20.0), 2.0);

    // Top and bottom run the full outer width; the sides only span the box.
    assert_eq!(bands[0], rect(98.0, 98.0, 54.0, 2.0));
    assert_eq!(bands[1], rect(98.0, 120.0, 54.0, 2.0));
    assert_eq!(bands[2], rect(98.0, 100.0, 2.0, 20.0));
    assert_eq!(bands[3], rect(150.0, 100.0, 2.0, 20.0));
}

#[test]
fn border_mode_moves_the_bands_across_the_edge_without_moving_the_box() {
    let box_ = rect(100.0, 100.0, 50.0, 20.0);

    // `Middle` straddles the edge: half the width each side of it.
    let middle = outline(&inset(&box_, 2.0), 4.0);
    assert_eq!(middle[0], rect(98.0, 98.0, 54.0, 4.0));
    assert_eq!(middle[1], rect(98.0, 118.0, 54.0, 4.0));

    // `Inset` sits wholly inside, its outer edge on the box's own.
    let inner = outline(&inset(&box_, 4.0), 4.0);
    assert_eq!(inner[0], rect(100.0, 100.0, 50.0, 4.0));
    assert_eq!(inner[1], rect(100.0, 116.0, 50.0, 4.0));
}

#[test]
fn a_transparent_background_hides_the_border_with_it() {
    let mut hidden = element(rect(0.0, 0.0, 10.0, 10.0), None);
    hidden.background_alpha = 0.0;
    hidden.border = Some((2.0, [0.0, 0.0, 0.0]));

    let (vertices, _, _) = build(&[hidden], &HashMap::new(), VIEWPORT);

    assert!(vertices.is_empty());
}

#[test]
fn a_background_and_its_border_are_five_quads_in_one_run() {
    let mut outlined = element(rect(0.0, 0.0, 10.0, 10.0), None);
    outlined.border = Some((1.0, [0.0, 0.0, 0.0]));

    let (vertices, runs, _) = build(&[outlined], &HashMap::new(), VIEWPORT);

    assert_eq!(vertices.len(), 5 * 6);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].texture, WHITE);
}

#[test]
fn consecutive_elements_sharing_a_texture_and_a_scissor_merge_into_one_run() {
    let elements = [
        element(rect(0.0, 0.0, 10.0, 10.0), None),
        element(rect(20.0, 0.0, 10.0, 10.0), None),
        element(
            rect(40.0, 0.0, 10.0, 10.0),
            Some(rect(0.0, 0.0, 100.0, 100.0)),
        ),
    ];

    let (_, runs, _) = build(&elements, &HashMap::new(), VIEWPORT);

    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].range, 0..12);
    assert_eq!(runs[1].range, 12..18);
}

fn stretch_image(asset: AssetRef) -> Painted {
    Painted {
        asset,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        repeat: [1.0, 1.0],
        scale: GuiImageScale::Stretch,
        rect_offset: [0.0, 0.0],
        rect_size: [0.0, 0.0],
        pixelated: false,
    }
}

#[test]
fn an_image_whose_asset_never_downloaded_leaves_only_its_background() {
    let mut label = element(rect(0.0, 0.0, 10.0, 10.0), None);
    label.image = Some(stretch_image(AssetRef::Id(7)));

    let (vertices, runs, _) = build(&[label], &HashMap::new(), VIEWPORT);

    assert_eq!(vertices.len(), 6);
    assert_eq!(runs.len(), 1);
}

#[test]
fn a_tiled_image_carries_its_repeat_count_into_the_uvs() {
    let mut label = element(rect(0.0, 0.0, 300.0, 200.0), None);
    label.background_alpha = 0.0;
    label.image = Some(Painted {
        scale: GuiImageScale::Tile,
        repeat: [3.0, 2.0],
        ..stretch_image(AssetRef::Id(7))
    });
    let textures = HashMap::from([(
        AssetRef::Id(7),
        Slot {
            linear: 1,
            nearest: 2,
            size: [64.0, 64.0],
        },
    )]);

    let (vertices, runs, _) = build(&[label], &textures, VIEWPORT);

    assert_eq!(runs[0].texture, 1);
    let corners: Vec<[f32; 2]> = vertices.iter().map(|vertex| vertex.uv).collect();
    assert!(corners.contains(&[0.0, 0.0]));
    assert!(corners.contains(&[3.0, 2.0]));
}

#[test]
fn a_pixelated_image_draws_through_the_nearest_slot_instead_of_linear() {
    let mut label = element(rect(0.0, 0.0, 10.0, 10.0), None);
    label.background_alpha = 0.0;
    label.image = Some(Painted {
        pixelated: true,
        ..stretch_image(AssetRef::Id(7))
    });
    let textures = HashMap::from([(
        AssetRef::Id(7),
        Slot {
            linear: 1,
            nearest: 2,
            size: [64.0, 64.0],
        },
    )]);

    let (_, runs, _) = build(&[label], &textures, VIEWPORT);

    assert_eq!(runs[0].texture, 2);
}

#[test]
fn a_zero_sized_element_draws_nothing() {
    let elements = [element(rect(10.0, 10.0, 0.0, 50.0), None)];

    let (vertices, _, _) = build(&elements, &HashMap::new(), VIEWPORT);

    assert!(vertices.is_empty());
}

#[test]
fn zero_rotation_is_the_identity() {
    let spin = Spin::new(0.0, [10.0, 5.0]);

    assert_eq!(spin.apply([0.0, 0.0]), [0.0, 0.0]);
    assert_eq!(spin.apply([12.0, 30.0]), [12.0, 30.0]);
}

#[test]
fn rotation_turns_a_point_clockwise_about_the_pivot() {
    // A square centred on the pivot: turning it 90 degrees clockwise sends
    // the point that was top-left to where top-right used to be — "up" swings
    // to "right" the way a clock's hand does, in this y-down pixel space.
    let spin = Spin::new(90.0, [10.0, 10.0]);
    let top_left = [5.0, 5.0];
    let top_right = [15.0, 5.0];

    let rotated = spin.apply(top_left);

    assert!((rotated[0] - top_right[0]).abs() < 1e-4);
    assert!((rotated[1] - top_right[1]).abs() < 1e-4);
}

#[test]
fn a_rotated_quad_stays_centred_on_the_unrotated_rects_centre() {
    // `GuiObject.Rotation` always turns about the element's own centre and
    // gives no way to move that pivot (not even via `AnchorPoint`), per
    // Roblox's own docs for the property.
    let mut spun = element(rect(0.0, 0.0, 20.0, 10.0), None);
    spun.rotation = 90.0;

    let (vertices, _, _) = build(&[spun], &HashMap::new(), VIEWPORT);

    let centre_x = vertices.iter().map(|v| v.position[0]).sum::<f32>() / vertices.len() as f32;
    let centre_y = vertices.iter().map(|v| v.position[1]).sum::<f32>() / vertices.len() as f32;
    assert!((centre_x - 10.0).abs() < 1e-3);
    assert!((centre_y - 5.0).abs() < 1e-3);
    // Rotated a quarter turn, the box's own axes swap: it now spans as far
    // vertically as it used to horizontally.
    let min_y = vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::MAX, f32::min);
    let max_y = vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::MIN, f32::max);
    assert!((max_y - min_y - 20.0).abs() < 1e-3);
}

#[test]
fn a_rotated_borders_bands_turn_about_the_elements_centre_too() {
    // `outline`'s four bands sit outside `rect`, each with its own, different
    // centre — sharing the element's own `Spin` (rather than each band
    // getting its own, freshly computed one) is what keeps them turning as
    // one rigid box rather than four independently-spinning strips.
    let mut bordered = element(rect(0.0, 0.0, 20.0, 10.0), None);
    bordered.rotation = 90.0;
    bordered.border = Some((4.0, [0.0, 0.0, 0.0]));

    let (vertices, _, _) = build(&[bordered], &HashMap::new(), VIEWPORT);
    // The background is the first quad (6 vertices); every vertex after that
    // is one of the four border bands.
    let border = &vertices[6..];

    let centre_x = border.iter().map(|v| v.position[0]).sum::<f32>() / border.len() as f32;
    let centre_y = border.iter().map(|v| v.position[1]).sum::<f32>() / border.len() as f32;
    assert!((centre_x - 10.0).abs() < 1e-3);
    assert!((centre_y - 5.0).abs() < 1e-3);

    // Unrotated, the bordered box spans 20 + 2*4 = 28 px horizontally and
    // 10 + 2*4 = 18 px vertically. A quarter turn swaps those: if every band
    // shares the element's own pivot, the vertical extent afterwards is the
    // *un*rotated horizontal one, 28 — not 18, and not something else
    // entirely, which is what four bands rotating about their own separate
    // centres would produce instead.
    let min_y = border
        .iter()
        .map(|v| v.position[1])
        .fold(f32::MAX, f32::min);
    let max_y = border
        .iter()
        .map(|v| v.position[1])
        .fold(f32::MIN, f32::max);
    assert!((max_y - min_y - 28.0).abs() < 1e-3);
}

mod modifiers;
