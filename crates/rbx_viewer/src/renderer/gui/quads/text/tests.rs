//! Unit tests for [`super`], against whatever fonts the machine has: none of
//! them assume a glyph's shape, only where the quads land and how many there
//! are. A machine with no fonts at all shapes nothing and skips them.

use std::collections::HashMap;

use super::super::build;
use super::*;
use crate::fonts::Face;
use crate::scene::{GuiElement, GuiTextSpan};

const VIEWPORT: (u32, u32) = (800, 600);

fn text(content: &str) -> GuiText {
    GuiText {
        spans: vec![GuiTextSpan::plain(content)],
        color: [1.0, 0.5, 0.0],
        alpha: 1.0,
        size: 20.0,
        scaled: false,
        wrapped: false,
        x_align: GuiAlign::Center,
        y_align: GuiAlign::Center,
        face: Face::default(),
        line_height: 1.0,
        stroke: None,
        truncate: false,
        max_graphemes: None,
        automatic: [false, false],
        size_bounds: None,
    }
}

fn element(text: GuiText) -> GuiElement {
    GuiElement {
        rect: GuiRect {
            x: 10.3,
            y: 20.7,
            width: 200.0,
            height: 50.0,
        },
        clip: None,
        rotation: 0.0,
        background: [0.0; 3],
        background_alpha: 0.0,
        border: None,
        border_inset: 0.0,
        z_index: 1,
        image: None,
        corner_radii: [0.0; 4],
        stroke: None,
        gradient: None,
        text: Some(GuiTypeset { text, size: 20.0 }),
        group: None,
    }
}

/// The vertices of every glyph run, or `None` where the machine shaped no
/// glyph at all.
fn glyph_vertices(elements: &[GuiElement], fonts: &mut Typesetter) -> Option<Vec<VertexRaw>> {
    let (vertices, runs, _) = build(elements, &HashMap::new(), VIEWPORT, fonts);
    let glyphs: Vec<VertexRaw> = runs
        .iter()
        .filter(|run| run.texture == GLYPHS)
        .flat_map(|run| vertices[run.range.start as usize..run.range.end as usize].to_vec())
        .collect();
    (!glyphs.is_empty()).then_some(glyphs)
}

#[test]
fn glyph_quads_sit_on_whole_pixels_inside_the_box_with_the_text_colour() {
    let mut fonts = Typesetter::new();
    let element = element(text("Hi"));

    let Some(vertices) = glyph_vertices(std::slice::from_ref(&element), &mut fonts) else {
        return;
    };

    assert_eq!(vertices.len() % 6, 0, "six vertices a glyph");
    for vertex in &vertices {
        assert_eq!(vertex.position[0].fract(), 0.0, "{vertex:?}");
        assert_eq!(vertex.position[1].fract(), 0.0, "{vertex:?}");
        assert_eq!(vertex.color, [1.0, 0.5, 0.0]);
        assert_eq!(vertex.alpha, 1.0);
        assert!(vertex.uv.iter().all(|uv| (0.0..=1.0).contains(uv)));
    }
    // Centred: the run straddles the box's middle on both axes.
    let rect = element.rect;
    let xs = vertices.iter().map(|vertex| vertex.position[0]);
    let ys = vertices.iter().map(|vertex| vertex.position[1]);
    let (left, right) = (
        xs.clone().fold(f32::MAX, f32::min),
        xs.fold(f32::MIN, f32::max),
    );
    let (top, bottom) = (
        ys.clone().fold(f32::MAX, f32::min),
        ys.fold(f32::MIN, f32::max),
    );
    let centre = [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5];
    assert!(left < centre[0] && centre[0] < right, "{left}..{right}");
    assert!(top < centre[1] && centre[1] < bottom, "{top}..{bottom}");
    assert!(left >= rect.x && right <= rect.x + rect.width);
}

#[test]
fn alignment_moves_the_run_to_the_box_edge() {
    let mut fonts = Typesetter::new();
    let mut left = text("Hi");
    left.x_align = GuiAlign::Start;
    left.y_align = GuiAlign::Start;
    let mut right = text("Hi");
    right.x_align = GuiAlign::End;
    right.y_align = GuiAlign::End;

    let Some(start) = glyph_vertices(&[element(left)], &mut fonts) else {
        return;
    };
    let end = glyph_vertices(&[element(right)], &mut fonts).unwrap();

    let leftmost = |vertices: &[VertexRaw]| {
        vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::MAX, f32::min)
    };
    let topmost = |vertices: &[VertexRaw]| {
        vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MAX, f32::min)
    };
    assert!(leftmost(&start) < leftmost(&end));
    assert!(topmost(&start) < topmost(&end));
    assert!(
        leftmost(&start) >= 10.0,
        "left-aligned text starts at the box"
    );
}

#[test]
fn a_stroke_draws_the_run_eight_more_times_one_pixel_out_under_the_fill() {
    let mut fonts = Typesetter::new();
    let plain = element(text("Hi"));
    let mut stroked = text("Hi");
    stroked.stroke = Some(([0.0, 0.0, 1.0], 0.5));
    let stroked = element(stroked);

    let Some(fill) = glyph_vertices(std::slice::from_ref(&plain), &mut fonts) else {
        return;
    };
    let all = glyph_vertices(std::slice::from_ref(&stroked), &mut fonts).unwrap();

    assert_eq!(all.len(), fill.len() * 9);
    let (outline, over) = all.split_at(fill.len() * 8);
    assert_eq!(over, &fill[..], "the fill comes last, unchanged");
    assert!(outline
        .iter()
        .all(|vertex| vertex.color == [0.0, 0.0, 1.0] && vertex.alpha == 0.5));
    // The first pass is the (-1, -1) offset of the fill.
    for (shifted, original) in outline.iter().zip(&fill) {
        assert_eq!(shifted.position[0], original.position[0] - 1.0);
        assert_eq!(shifted.position[1], original.position[1] - 1.0);
    }
}

#[test]
fn text_turns_with_its_element_and_keeps_its_scissor() {
    let mut fonts = Typesetter::new();
    let upright = element(text("Hi"));
    let mut turned = element(text("Hi"));
    turned.rotation = 180.0;
    turned.clip = Some(GuiRect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    });

    let Some(before) = glyph_vertices(std::slice::from_ref(&upright), &mut fonts) else {
        return;
    };
    let (vertices, runs, _) = build(
        std::slice::from_ref(&turned),
        &HashMap::new(),
        VIEWPORT,
        &mut fonts,
    );
    let run = runs.iter().find(|run| run.texture == GLYPHS).unwrap();
    assert_eq!(
        run.scissor,
        Some(Scissor {
            x: 0,
            y: 0,
            width: 100,
            height: 100
        })
    );

    // A half turn about the box's centre mirrors every corner through it.
    let rect = upright.rect;
    let centre = [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5];
    let after = &vertices[run.range.start as usize..run.range.end as usize];
    for (turned, upright) in after.iter().zip(&before) {
        for (axis, centre) in centre.iter().enumerate() {
            let mirrored = 2.0 * centre - upright.position[axis];
            assert!((turned.position[axis] - mirrored).abs() < 1e-3);
        }
    }
}

#[test]
fn max_visible_graphemes_hides_the_tail_without_moving_the_head() {
    let mut fonts = Typesetter::new();
    let mut limited = text("Hello");
    limited.max_graphemes = Some(2);
    limited.x_align = GuiAlign::Start;
    let mut whole = text("Hello");
    whole.x_align = GuiAlign::Start;

    let Some(all) = glyph_vertices(&[element(whole)], &mut fonts) else {
        return;
    };
    let some = glyph_vertices(&[element(limited)], &mut fonts).unwrap();

    assert!(some.len() < all.len());
    assert_eq!(&all[..some.len()], &some[..], "the first glyphs stay put");
}

#[test]
fn transparent_text_and_a_blank_string_emit_nothing() {
    let mut fonts = Typesetter::new();
    let mut invisible = text("Hi");
    invisible.alpha = 0.0;

    assert!(glyph_vertices(&[element(invisible)], &mut fonts).is_none());
    assert!(glyph_vertices(&[element(text("   "))], &mut fonts).is_none());
}

#[test]
fn wrapped_lines_past_the_box_are_dropped() {
    let mut fonts = Typesetter::new();
    let mut tall = text("one two three four five six seven eight nine ten");
    tall.wrapped = true;
    tall.x_align = GuiAlign::Start;
    tall.y_align = GuiAlign::Start;
    let mut narrow = element(tall);
    narrow.rect.width = 60.0;
    narrow.rect.height = 25.0;

    let Some(vertices) = glyph_vertices(std::slice::from_ref(&narrow), &mut fonts) else {
        return;
    };

    let bottom = vertices
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::MIN, f32::max);
    // One 20-pixel line fits in 25; the rest is not drawn at all.
    assert!(bottom <= narrow.rect.y + 25.0 + 4.0, "bottom {bottom}");
}
