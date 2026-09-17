//! What a `UICorner`, `UIStroke` or `UIGradient` puts on the vertices: the
//! element's own frame for the distance field, the stroke band, the ramp row.

use super::*;
use crate::scene::{GuiGradient, GuiGradientKind, GuiJoin, GuiStroke, GuiTile};
use rbx_dom::{Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence};

fn rounded(radius: f32) -> GuiElement {
    let mut element = element(rect(100.0, 50.0, 200.0, 100.0), None);
    element.corner_radii = [radius; 4];
    element
}

fn stroke(band: [f32; 2], join: GuiJoin) -> GuiStroke {
    GuiStroke {
        color: [0.0, 1.0, 0.0],
        alpha: 1.0,
        band,
        join,
        on_text: false,
    }
}

fn gradient() -> GuiGradient {
    GuiGradient {
        color: ColorSequence {
            keypoints: vec![ColorSequenceKeypoint {
                time: 0.0,
                color: Color3Data {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                },
                envelope: 0.0,
            }],
        },
        transparency: NumberSequence { keypoints: vec![] },
        origin: [10.0, -5.0],
        axis: [0.005, 0.0],
        kind: GuiGradientKind::Conical,
        tile: GuiTile::Repeat,
    }
}

// The fragment's distance field is evaluated in the element's own frame, so
// every vertex carries where it sits relative to the box centre *before*
// rotation — which is what keeps a rotated rounded box round.
#[test]
fn a_vertex_carries_its_unrotated_position_relative_to_the_centre() {
    let mut spun = rounded(8.0);
    spun.rotation = 90.0;

    let (vertices, _, _) = build(&[spun], &HashMap::new(), VIEWPORT);

    let locals: Vec<[f32; 2]> = vertices.iter().map(|vertex| vertex.local).collect();
    assert!(locals.contains(&[-100.0, -50.0]));
    assert!(locals.contains(&[100.0, 50.0]));
    assert!(vertices
        .iter()
        .all(|vertex| vertex.half == [100.0, 50.0] && vertex.radii == [8.0; 4]));
    assert!(vertices.iter().all(|vertex| vertex.band == FILL));
}

// A sharp box is left to the rasterizer: its half-size is pushed so far out
// that the distance field never reaches a pixel.
#[test]
fn a_sharp_box_is_shaped_by_nothing() {
    let (vertices, _, _) = build(&[rounded(0.0)], &HashMap::new(), VIEWPORT);

    assert!(vertices
        .iter()
        .all(|vertex| vertex.half[0] > 1_000.0 && vertex.radii == [0.0; 4]));
}

#[test]
fn a_stroke_is_one_more_quad_grown_by_its_outer_edge_and_a_pixel_of_ramp() {
    let mut outlined = rounded(8.0);
    outlined.stroke = Some(stroke([0.0, 6.0], GuiJoin::Round));

    let (vertices, runs, _) = build(&[outlined], &HashMap::new(), VIEWPORT);

    assert_eq!(vertices.len(), 12);
    // Background and stroke share the white texture and one run.
    assert_eq!(runs.len(), 1);
    let band = &vertices[6..];
    let min_x = band.iter().map(|v| v.position[0]).fold(f32::MAX, f32::min);
    let max_x = band.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
    assert_eq!((min_x, max_x), (93.0, 307.0));
    // The band keeps the element's real outline, not the sharp box's
    // unbounded one, and carries the band it is clipped to.
    assert!(band
        .iter()
        .all(|v| v.half == [100.0, 50.0] && v.band == [0.0, 6.0] && v.color == [0.0, 1.0, 0.0]));
}

#[test]
fn an_inner_stroke_does_not_grow_the_quad() {
    let mut outlined = rounded(0.0);
    outlined.stroke = Some(stroke([-4.0, 0.0], GuiJoin::Round));

    let (vertices, _, _) = build(&[outlined], &HashMap::new(), VIEWPORT);

    let min_x = vertices[6..]
        .iter()
        .map(|v| v.position[0])
        .fold(f32::MAX, f32::min);
    assert_eq!(min_x, 99.0);
}

// A hollow box: the stroke draws with no background at all, unlike the
// pixel border, which the background's transparency takes with it.
#[test]
fn a_stroke_draws_without_a_background() {
    let mut hollow = rounded(0.0);
    hollow.background_alpha = 0.0;
    hollow.stroke = Some(stroke([0.0, 2.0], GuiJoin::Bevel));

    let (vertices, _, _) = build(&[hollow], &HashMap::new(), VIEWPORT);

    assert_eq!(vertices.len(), 6);
    assert_eq!(vertices[0].mode & 3, GuiJoin::Bevel as u32);
}

#[test]
fn a_stroke_on_text_is_left_to_the_text_renderer() {
    let mut labelled = rounded(0.0);
    labelled.stroke = Some(GuiStroke {
        on_text: true,
        ..stroke([0.0, 2.0], GuiJoin::Round)
    });

    let (vertices, _, _) = build(&[labelled], &HashMap::new(), VIEWPORT);

    assert_eq!(vertices.len(), 6);
}

#[test]
fn a_gradient_is_baked_to_a_row_the_fills_refer_to_and_the_stroke_does_not() {
    let mut shaded = rounded(0.0);
    shaded.gradient = Some(gradient());
    shaded.stroke = Some(stroke([0.0, 2.0], GuiJoin::Round));

    let (vertices, _, rows) = build(&[shaded], &HashMap::new(), VIEWPORT);

    assert_eq!(rows.len(), 1);
    let fill = &vertices[0];
    assert_eq!(fill.gradient_row, 0.0);
    assert_eq!(fill.gradient, [10.0, -5.0, 0.005, 0.0]);
    assert_eq!((fill.mode >> 2) & 3, GuiGradientKind::Conical as u32);
    assert_eq!((fill.mode >> 4) & 3, GuiTile::Repeat as u32);
    assert_eq!(vertices[6].gradient_row, -1.0);
}

#[test]
fn two_elements_with_the_same_ramp_share_a_row() {
    let mut first = rounded(0.0);
    first.gradient = Some(gradient());
    let second = first.clone();

    let (vertices, _, rows) = build(&[first, second], &HashMap::new(), VIEWPORT);

    assert_eq!(rows.len(), 1);
    assert!(vertices.iter().all(|vertex| vertex.gradient_row == 0.0));
}

#[test]
fn the_mode_packs_three_ordinals_without_overlap() {
    let mode = crate::renderer::gui::pipeline::mode(2, 1, 2);

    assert_eq!(mode & 3, 2);
    assert_eq!((mode >> 2) & 3, 1);
    assert_eq!((mode >> 4) & 3, 2);
}
