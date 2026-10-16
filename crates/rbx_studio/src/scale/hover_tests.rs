//! The hover fade `install` turns on (the vendored GPUI's
//! `hover_transition`): its blend arithmetic, and a real window easing one
//! hover state in and back out on GPUI's test clock.
//!
//! GPUI's test platform never delivers the frames a running fade asks for
//! (`request_animation_frame`), and the hook that would is crate-private, so
//! the window test draws each frame itself, as the display would.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{
    blend_refinement, canvas, div, mix_colors, point, px, BoxShadow, Context, CursorStyle,
    HoverFade, HoverTransition, Hsla, IntoElement, Length, Modifiers, Render, Rgba, Size, Style,
    StyleRefinement, TestAppContext, VisualTestContext, Window,
};

const FADE: Duration = Duration::from_millis(120);

fn rgb(r: f32, g: f32, b: f32) -> Hsla {
    Rgba { r, g, b, a: 1. }.into()
}

fn close(a: Hsla, b: Hsla) -> bool {
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    [a.r - b.r, a.g - b.g, a.b - b.b, a.a - b.a]
        .iter()
        .all(|d| d.abs() < 1e-3)
}

#[test]
fn a_fade_runs_from_rest_to_hovered_over_the_duration() {
    let start = Instant::now();
    let mut fade = HoverFade::default();
    assert_eq!(fade.progress(true, start, FADE), 0.);
    let midway = fade.progress(true, start + FADE / 2, FADE);
    assert!(
        midway > 0.5 && midway < 1.,
        "ease-out is past half at half time: {midway}"
    );
    assert_eq!(fade.progress(true, start + FADE, FADE), 1.);
    assert_eq!(fade.progress(true, start + FADE * 4, FADE), 1.);
}

#[test]
fn leaving_midway_reverses_from_where_the_fade_had_got_to() {
    let start = Instant::now();
    let mut fade = HoverFade::default();
    fade.progress(true, start, FADE);
    let reached = fade.progress(false, start + FADE / 4, FADE);
    assert!(reached > 0. && reached < 1.);
    // No jump at the turn, and the way back takes only the share of the
    // duration the way in had covered.
    let after = fade.progress(false, start + FADE / 4 + Duration::from_millis(1), FADE);
    assert!(after < reached && reached - after < 0.1);
    let back = start + FADE / 4 + FADE.mul_f32(reached);
    assert_eq!(fade.progress(false, back, FADE), 0.);
}

#[test]
fn mixing_from_clear_changes_only_the_alpha() {
    let to = rgb(0.2, 0.4, 0.8);
    let half = Rgba::from(mix_colors(to.opacity(0.), to, 0.5));
    assert!((half.a - 0.5).abs() < 1e-3);
    assert!(
        (half.r - 0.2).abs() < 1e-3 && (half.g - 0.4).abs() < 1e-3 && (half.b - 0.8).abs() < 1e-3
    );
    assert!(close(
        mix_colors(rgb(1., 0., 0.), rgb(0., 0., 1.), 0.),
        rgb(1., 0., 0.)
    ));
    assert!(close(
        mix_colors(rgb(1., 0., 0.), rgb(0., 0., 1.), 1.),
        rgb(0., 0., 1.)
    ));
    assert!(close(
        mix_colors(rgb(1., 0., 0.), rgb(0., 0., 1.), 0.5),
        rgb(0.5, 0., 0.5)
    ));
}

#[test]
fn a_blend_eases_colours_and_holds_the_rest_only_while_hovered() {
    let hover = StyleRefinement::default()
        .bg(rgb(0., 0., 1.))
        .border_color(rgb(0., 1., 0.))
        .cursor_pointer();
    let base = Style {
        background: Some(rgb(1., 0., 0.).into()),
        ..Style::default()
    };

    let going = blend_refinement(&base, &hover, 0.5, true);
    let fill = going
        .background
        .as_ref()
        .and_then(|f| f.color()?.as_solid());
    assert!(close(fill.unwrap(), rgb(0.5, 0., 0.5)));
    // The border had no resting colour, so it fades in from clear.
    assert!(close(
        going.border_color.unwrap(),
        rgb(0., 1., 0.).opacity(0.5)
    ));
    assert_eq!(going.mouse_cursor, Some(CursorStyle::PointingHand));

    let leaving = blend_refinement(&base, &hover, 0.5, false);
    assert!(leaving.background.is_some());
    assert_eq!(leaving.mouse_cursor, None);
}

#[test]
fn an_inherited_text_colour_switches_halfway() {
    let hover = StyleRefinement::default().text_color(rgb(1., 1., 1.));
    let base = Style::default();
    assert_eq!(blend_refinement(&base, &hover, 0.4, true).text.color, None);
    assert_eq!(
        blend_refinement(&base, &hover, 0.6, true).text.color,
        Some(rgb(1., 1., 1.))
    );
}

#[test]
fn a_glow_with_no_resting_shadow_fades_in() {
    let glow = BoxShadow {
        color: rgb(1., 1., 0.),
        offset: point(px(0.), px(0.)),
        blur_radius: px(6.),
        spread_radius: px(0.),
        inset: false,
    };
    let hover = StyleRefinement::default().shadow(vec![glow.clone()]);
    let shadows = blend_refinement(&Style::default(), &hover, 0.25, true)
        .box_shadow
        .unwrap();
    assert_eq!(shadows.len(), 1);
    assert!((shadows[0].color.a - 0.25).abs() < 1e-3);
    assert_eq!(shadows[0].blur_radius, glow.blur_radius);
}

#[test]
fn a_pixel_size_lift_eases_and_a_fractional_one_holds() {
    let base = Style {
        size: Size {
            width: px(20.).into(),
            height: Length::Auto,
        },
        ..Style::default()
    };
    let hover = StyleRefinement::default().w(px(30.)).h_full();

    let going = blend_refinement(&base, &hover, 0.5, true);
    assert_eq!(going.size.width, Some(px(25.).into()));
    assert_eq!(
        going.size.height, hover.size.height,
        "a fraction holds while hovered"
    );

    let leaving = blend_refinement(&base, &hover, 0.5, false);
    assert_eq!(leaving.size.width, Some(px(25.).into()), "and eases back");
    assert_eq!(leaving.size.height, None);
}

struct Row {
    seen: Rc<Cell<Option<Hsla>>>,
}

const REST: Rgba = Rgba {
    r: 0.,
    g: 0.,
    b: 0.,
    a: 1.,
};
const HOVERED: Rgba = Rgba {
    r: 1.,
    g: 1.,
    b: 1.,
    a: 1.,
};

impl Render for Row {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let seen = self.seen.clone();
        // The text colour reaches children, so a canvas inside reads the
        // blend the row was painted with.
        div().size_full().child(
            div()
                .id("row")
                .size(px(100.))
                .text_color(REST)
                .hover(|this| this.text_color(HOVERED))
                .child(
                    canvas(
                        move |_, window, _| seen.set(window.text_style().color.into()),
                        |_, _, _, _| {},
                    )
                    .size_full(),
                ),
        )
    }
}

/// Hovers the row, then reads its colour `steps` times, `step` apart, on
/// GPUI's test clock; then leaves it and reads once more after `step`.
fn hover_trail(
    cx: &mut TestAppContext,
    transition: Option<Duration>,
    reduce_motion: bool,
    step: Duration,
    steps: usize,
) -> (Vec<Hsla>, Hsla) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(reduce_motion);
        if let Some(duration) = transition {
            cx.set_global(HoverTransition { duration });
        }
    });
    let seen = Rc::new(Cell::new(None));
    let (_view, cx) = cx.add_window_view({
        let seen = seen.clone();
        move |_, _| Row { seen }
    });
    let frame = |cx: &mut VisualTestContext, after: Duration| {
        cx.executor().advance_clock(after);
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        seen.get().unwrap()
    };
    cx.run_until_parked();
    cx.simulate_mouse_move(point(px(50.), px(50.)), None, Modifiers::none());
    let mut trail = vec![frame(cx, Duration::ZERO)];
    for _ in 0..steps {
        trail.push(frame(cx, step));
    }
    cx.simulate_mouse_move(point(px(500.), px(500.)), None, Modifiers::none());
    frame(cx, Duration::ZERO);
    (trail, frame(cx, step))
}

#[gpui_kit::test]
fn a_hovered_row_eases_to_its_hover_colour_and_back(cx: &mut TestAppContext) {
    let (trail, leaving) = hover_trail(cx, Some(FADE), false, FADE / 4, 6);
    assert!(
        close(trail[0], REST.into()),
        "starts at rest: {:?}",
        trail[0]
    );
    let lightness: Vec<f32> = trail.iter().map(|c| Rgba::from(*c).r).collect();
    assert!(
        lightness.windows(2).all(|w| w[1] >= w[0]),
        "only brightens: {lightness:?}"
    );
    assert!(
        lightness[1] > 0. && lightness[1] < 1.,
        "a quarter in, part way: {lightness:?}"
    );
    assert!(close(*trail.last().unwrap(), HOVERED.into()));
    let back = Rgba::from(leaving).r;
    assert!(back > 0. && back < 1., "leaving fades too: {back}");
}

#[gpui_kit::test]
fn without_a_transition_hover_snaps_as_upstream_does(cx: &mut TestAppContext) {
    let (trail, leaving) = hover_trail(cx, None, false, FADE / 4, 1);
    assert!(close(trail[0], HOVERED.into()));
    assert!(close(leaving, REST.into()));
}

#[gpui_kit::test]
fn reduced_motion_snaps_hover_even_with_a_transition(cx: &mut TestAppContext) {
    let (trail, leaving) = hover_trail(cx, Some(FADE), true, FADE / 4, 1);
    assert!(close(trail[0], HOVERED.into()));
    assert!(close(leaving, REST.into()));
}
