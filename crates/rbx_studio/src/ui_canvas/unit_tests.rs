//! The Offset/Scale switch, Alt's centred resize, zoom to selection, and
//! placing a box in a new parent.

use super::arrange::{place, Member};
use super::*;

#[test]
fn offset_moves_pixels_and_scale_moves_a_share_of_the_span() {
    let udim = [(0.25, 10), (0.0, 4)];
    assert_eq!(
        shifted_in(udim, [5.4, -2.0], Unit::Offset, [200.0, 100.0]),
        [(0.25, 15), (0.0, 2)]
    );
    assert_eq!(
        shifted_in(udim, [50.0, -25.0], Unit::Scale, [200.0, 100.0]),
        [(0.5, 10), (-0.25, 4)]
    );
    // No span to measure a share of: pixels, whatever the switch says.
    assert_eq!(
        shifted_in(udim, [5.0, 0.0], Unit::Scale, [0.0, 100.0]),
        [(0.25, 15), (0.0, 4)]
    );
}

#[test]
fn a_scale_keeps_four_places() {
    assert_eq!(round_scale(1.0 / 3.0), 0.3333);
    assert_eq!(round_scale(0.00004), 0.0);
}

#[test]
fn a_centred_resize_moves_both_sides_and_never_turns_the_box_inside_out() {
    let step = resize(Handle(1, 0), [100.0, 50.0], [10.0, 0.0], false);
    assert_eq!(
        centred(step, [100.0, 50.0]),
        Resize {
            grow: [20.0, 0.0],
            centre: [0.0, 0.0]
        }
    );
    let shrink = resize(Handle(1, 0), [100.0, 50.0], [-80.0, 0.0], false);
    assert_eq!(centred(shrink, [100.0, 50.0]).grow, [-100.0, 0.0]);
}

#[test]
fn framing_centres_a_box_and_may_zoom_past_one_to_one() {
    let rect = Rect {
        x: 100.0,
        y: 100.0,
        w: 50.0,
        h: 20.0,
    };
    let view = View::framing([400.0, 300.0], &rect, 50.0);
    assert_eq!(view.zoom, 6.0);
    assert_eq!(view.to_view(rect.centre()), [200.0, 150.0]);
}

#[test]
fn placing_a_box_keeps_it_where_it_is_in_the_mode_asked_for() {
    let member = Member {
        rect: Rect {
            x: 60.0,
            y: 30.0,
            w: 40.0,
            h: 20.0,
        },
        anchor: [0.5, 0.5],
        position: [(0.0, 0); 2],
        size: [(0.0, 0); 2],
        size_scale: 1.0,
        size_axes: [0, 1],
    };
    let pixels = place(&member, [10.0, 10.0], [200.0, 100.0], [[false; 2]; 2]);
    assert_eq!(pixels, ([(0.0, 70), (0.0, 30)], [(0.0, 40), (0.0, 20)]));
    let scaled = place(&member, [10.0, 10.0], [200.0, 100.0], [[true; 2]; 2]);
    assert_eq!(scaled, ([(0.35, 0), (0.3, 0)], [(0.2, 0), (0.2, 0)]));
}
