use rbx_dom::Ref;
use rbx_viewer::GuiBox;

use super::arrange::{self, Grouping, Member};
use super::carry::{self, Carried};
use super::guides::{self, Guide};
use super::*;
use crate::align::Mode;

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

fn placed(id: u32, r: Rect, rotation: f32) -> GuiBox {
    GuiBox {
        referent: Ref::new(id),
        rect: [r.x, r.y, r.w, r.h],
        rotation,
        content: Some([r.x, r.y, r.w, r.h]),
        size_scale: 1.0,
        aspect: false,
    }
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
}

#[test]
fn a_click_lands_on_the_topmost_element_under_it() {
    let boxes = [
        placed(1, rect(0.0, 0.0, 100.0, 100.0), 0.0),
        placed(2, rect(10.0, 10.0, 20.0, 20.0), 0.0),
    ];
    assert_eq!(hit(&boxes, [15.0, 15.0]), Some(Ref::new(2)));
    assert_eq!(hit(&boxes, [50.0, 50.0]), Some(Ref::new(1)));
    assert_eq!(hit(&boxes, [150.0, 50.0]), None);
}

// A 100×10 bar turned a quarter stands 10 wide and 100 tall about the same
// centre: a point off its unturned end is off it now, one above the centre
// is on it.
#[test]
fn a_turned_element_is_hit_where_it_is_drawn() {
    let bar = placed(1, rect(0.0, 45.0, 100.0, 10.0), 90.0);
    assert!(!covers(&bar, [5.0, 50.0]));
    assert!(covers(&bar, [50.0, 5.0]));
}

#[test]
fn a_marquee_takes_the_outermost_of_what_it_wholly_holds() {
    let boxes = [
        placed(1, rect(0.0, 0.0, 50.0, 50.0), 0.0),
        placed(2, rect(10.0, 10.0, 10.0, 10.0), 0.0),
        placed(3, rect(60.0, 0.0, 50.0, 50.0), 0.0),
    ];
    let parent = |r: Ref| (r == Ref::new(2)).then_some(Ref::new(1));
    assert_eq!(
        marquee(&boxes, rect(-5.0, -5.0, 70.0, 70.0), parent),
        [Ref::new(1)],
        "the card, not its label; the half-covered box stays out"
    );
    assert_eq!(
        marquee(&boxes, rect(5.0, 5.0, 20.0, 20.0), parent),
        [Ref::new(2)]
    );
}

#[test]
fn a_side_handle_grows_one_side_and_moves_the_centre_half_as_far() {
    let right = resize(Handle(1, 0), [100.0, 50.0], [10.0, 99.0], false);
    assert_eq!(right.grow, [10.0, 0.0]);
    assert_eq!(right.centre, [5.0, 0.0]);

    let left = resize(Handle(-1, 0), [100.0, 50.0], [10.0, 0.0], false);
    assert_eq!(left.grow, [-10.0, 0.0], "dragged inwards");
    assert_eq!(left.centre, [5.0, 0.0]);
}

#[test]
fn a_box_never_turns_inside_out() {
    let squashed = resize(Handle(1, 1), [100.0, 50.0], [-500.0, -500.0], false);
    assert_eq!(squashed.grow, [-100.0, -50.0]);
}

#[test]
fn a_corner_with_the_aspect_kept_scales_both_sides_by_one_factor() {
    let step = resize(Handle(1, 1), [100.0, 50.0], [50.0, 5.0], true);
    assert_eq!(step.grow, [50.0, 25.0]);
}

// `Position` is where `AnchorPoint` lands: growing a centred box by 20 about
// a fixed left edge moves its centre 10 right, and its anchor with it; a
// top-left anchored one, whose anchor is the fixed edge, does not move.
#[test]
fn the_position_follows_the_anchor_not_the_corner() {
    assert_eq!(
        position_shift([10.0, 0.0], 0.0, [0.5, 0.5], [20.0, 0.0]),
        [10.0, 0.0]
    );
    assert_eq!(
        position_shift([10.0, 0.0], 0.0, [0.0, 0.0], [20.0, 0.0]),
        [0.0, 0.0]
    );
}

// A parent turned a quarter clockwise: a move straight down on screen is
// along the parent's own +x.
#[test]
fn a_move_is_written_in_the_turned_parents_own_frame() {
    let shift = position_shift([0.0, 10.0], 90.0, [0.0, 0.0], [0.0, 0.0]);
    assert!(close(shift, [10.0, 0.0]), "{shift:?}");
}

#[test]
fn offsets_shift_and_round_and_keep_their_scale() {
    assert_eq!(
        shifted([(0.5, 10), (0.0, -3)], [2.6, -0.4]),
        [(0.5, 13), (0.0, -3)]
    );
    assert_eq!(udim2_text([(0.5, 13), (0.0, -3)]), "0.5, 13, 0, -3");
}

#[test]
fn handles_sit_on_the_turned_box() {
    let r = rect(0.0, 0.0, 100.0, 50.0);
    assert!(close(Handle(1, 0).at(&r, 0.0), [100.0, 25.0]));
    assert!(close(Handle(1, 0).at(&r, 90.0), [50.0, 75.0]));
}

#[test]
fn a_move_snaps_its_nearest_line_onto_a_siblings_within_reach() {
    let sibling = rect(100.0, 0.0, 50.0, 50.0);
    // Left edge 97 is 3 off the sibling's 100 and the centre 117 is 8 off
    // its 125 — the nearer one wins.
    let shift = guides::snap_move(&rect(97.0, 200.0, 40.0, 40.0), &[sibling], 4.0);
    assert_eq!(shift, [3.0, 0.0], "down is out of reach");
    assert_eq!(
        guides::snap_move(&rect(90.0, 200.0, 5.0, 5.0), &[sibling], 4.0),
        [0.0, 0.0]
    );
}

#[test]
fn a_shared_line_draws_one_guide_across_both_boxes() {
    let moving = rect(100.0, 200.0, 10.0, 10.0);
    let found = guides::guides(&moving, &[rect(100.0, 0.0, 50.0, 50.0)]);
    assert_eq!(
        found,
        [Guide {
            axis: 0,
            at: 100.0,
            from: 0.0,
            to: 210.0,
        }]
    );
}

#[test]
fn the_readout_measures_gaps_between_boxes_and_insets_inside_one() {
    let gaps = guides::measure(&rect(0.0, 0.0, 10.0, 10.0), &rect(30.0, 5.0, 10.0, 10.0));
    assert_eq!(gaps.len(), 1, "they overlap down, so only across");
    assert_eq!(gaps[0].length, 20.0);
    assert_eq!(gaps[0].a, [10.0, 7.5]);

    let insets = guides::measure(&rect(10.0, 20.0, 10.0, 10.0), &rect(0.0, 0.0, 100.0, 100.0));
    let lengths: Vec<f32> = insets.iter().map(|d| d.length).collect();
    assert_eq!(lengths, [10.0, 80.0, 20.0, 70.0]);
}

#[test]
fn aligning_goes_through_the_align_tools_own_geometry() {
    let boxes = [
        (Ref::new(1), rect(0.0, 0.0, 10.0, 10.0)),
        (Ref::new(2), rect(40.0, 30.0, 20.0, 10.0)),
    ];
    let left = arrange::align(&boxes, 0, Mode::Min);
    assert_eq!(left, [(Ref::new(2), [-40.0, 0.0])]);
    let bottom = arrange::align(&boxes, 1, Mode::Max);
    assert_eq!(bottom, [(Ref::new(1), [0.0, 30.0])]);
}

#[test]
fn distributing_evens_out_the_gaps_between_the_ends() {
    let boxes = [
        rect(0.0, 0.0, 10.0, 10.0),
        rect(100.0, 0.0, 10.0, 10.0),
        rect(20.0, 0.0, 30.0, 10.0),
    ];
    // Span 0..110 holds 50 of boxes, so each of the two gaps is 30.
    assert_eq!(arrange::distribute(&boxes, 0), [0.0, 0.0, 20.0]);
    assert_eq!(arrange::distribute(&boxes[..2], 0), [0.0, 0.0]);
}

fn member(r: Rect, anchor: [f32; 2], position: Udim2, size: Udim2) -> Member {
    Member {
        rect: r,
        anchor,
        position,
        size,
        size_scale: 1.0,
        size_axes: [0, 1],
    }
}

// Two offset-placed members of a parent whose content box starts at
// (100, 100), one anchored at its centre: the frame fits round both and each
// keeps its place on screen.
#[test]
fn a_group_frame_fits_its_members_without_moving_them() {
    let members = [
        member(
            rect(110.0, 120.0, 20.0, 20.0),
            [0.0, 0.0],
            [(0.0, 10), (0.0, 20)],
            [(0.0, 20), (0.0, 20)],
        ),
        member(
            rect(150.0, 110.0, 10.0, 40.0),
            [0.5, 0.5],
            [(0.0, 55), (0.0, 30)],
            [(0.0, 10), (0.0, 40)],
        ),
    ];
    assert_eq!(
        arrange::group(&members, Some(rect(100.0, 100.0, 400.0, 300.0))),
        Some(Grouping {
            frame: ([(0.0, 10), (0.0, 10)], [(0.0, 50), (0.0, 40)]),
            members: vec![
                ([(0.0, 0), (0.0, 10)], [(0.0, 20), (0.0, 20)]),
                ([(0.0, 45), (0.0, 20)], [(0.0, 10), (0.0, 40)]),
            ],
        })
    );
}

// Scale stays scale — of the new frame for a member, of the parent for the
// frame — and pixels stay pixels; `UIScale` comes back out of a size, and a
// `RelativeXX` height is taken against the frame's width.
#[test]
fn a_group_keeps_each_value_in_the_mode_it_was_written_in() {
    let mut scaled = member(
        rect(250.0, 50.0, 50.0, 100.0),
        [0.0, 0.0],
        [(0.0, 250), (0.0, 50)],
        [(0.0, 25), (0.0, 50)],
    );
    scaled.size_scale = 2.0;
    let mut wide = member(
        rect(100.0, 150.0, 50.0, 50.0),
        [0.0, 0.0],
        [(0.0, 100), (0.0, 150)],
        [(0.0, 50), (0.25, 0)],
    );
    wide.size_axes = [0, 0];
    let members = [
        member(
            rect(100.0, 50.0, 100.0, 50.0),
            [0.0, 0.0],
            [(0.25, 0), (0.25, 0)],
            [(0.25, 0), (0.25, 0)],
        ),
        scaled,
        wide,
    ];
    let grouping = arrange::group(&members, Some(rect(0.0, 0.0, 400.0, 200.0))).unwrap();
    assert_eq!(
        grouping.frame,
        ([(0.25, 0), (0.25, 0)], [(0.5, 0), (0.75, 0)]),
        "a member used scale, so the frame does too"
    );
    assert_eq!(
        grouping.members,
        [
            ([(0.0, 0), (0.0, 0)], [(0.5, 0), (50.0 / 150.0, 0)]),
            ([(0.0, 150), (0.0, 0)], [(0.0, 25), (0.0, 50)]),
            ([(0.0, 0), (0.0, 100)], [(0.0, 50), (0.25, 0)]),
        ]
    );
}

// Inside a `ScrollingFrame` there is no content box to measure against: the
// origin comes back from the first member's pixels, and pixels go out.
#[test]
fn a_group_in_a_scrolled_canvas_is_written_in_pixels() {
    let members = [member(
        rect(130.0, 40.0, 10.0, 10.0),
        [0.0, 0.0],
        [(0.0, 30), (0.0, 40)],
        [(0.0, 10), (0.0, 10)],
    )];
    let grouping = arrange::group(&members, None).unwrap();
    assert_eq!(
        grouping.frame,
        ([(0.0, 30), (0.0, 40)], [(0.0, 10), (0.0, 10)])
    );
}

// One element carried by a frame that is its own box is exactly a plain
// resize of it: the same growth, the centre moved the same way.
#[test]
fn one_element_carried_is_its_own_resize() {
    let frame = rect(0.0, 0.0, 100.0, 50.0);
    let step = resize(Handle(1, 0), [100.0, 50.0], [10.0, 0.0], false);
    let moved = carry::scale(
        &frame,
        30.0,
        &step,
        &[Carried {
            centre: frame.centre(),
            size: [100.0, 50.0],
            rotation: 30.0,
        }],
    );
    assert!(close(moved[0].centre, rotate(step.centre, 30.0)));
    assert!(close(moved[0].grow, step.grow));
}

// A selection stretched half again across: each element's place scales from
// the fixed edge, and a quarter-turned one grows along its own height.
#[test]
fn a_selection_stretches_as_one() {
    let frame = rect(0.0, 0.0, 200.0, 100.0);
    let step = resize(Handle(1, 0), [200.0, 100.0], [100.0, 0.0], false);
    let carried = [
        Carried {
            centre: [50.0, 50.0],
            size: [100.0, 100.0],
            rotation: 0.0,
        },
        Carried {
            centre: [150.0, 50.0],
            size: [100.0, 100.0],
            rotation: 90.0,
        },
    ];
    let moved = carry::scale(&frame, 0.0, &step, &carried);
    assert!(close(moved[0].centre, [25.0, 0.0]));
    assert!(close(moved[0].grow, [50.0, 0.0]));
    assert!(close(moved[1].centre, [75.0, 0.0]));
    assert!(close(moved[1].grow, [0.0, 50.0]));
}

#[test]
fn a_selection_turns_about_one_pivot() {
    let carried = [Carried {
        centre: [150.0, 50.0],
        size: [10.0, 10.0],
        rotation: 0.0,
    }];
    let moved = carry::turn([100.0, 50.0], 90.0, &carried);
    assert!(close(moved[0], [-50.0, 50.0]));
}

// Half of a 400-wide parent is 200 pixels however it is spelt: offset in,
// scale out, and a scale already there keeps its share.
#[test]
fn an_offset_folds_into_the_scale_it_stands_for() {
    assert_eq!(
        arrange::to_scale([(0.0, 200), (0.25, 30)], [400.0, 300.0]),
        [(0.5, 0), (0.35, 0)]
    );
    assert_eq!(
        arrange::to_scale([(0.0, 50), (0.0, 10)], [0.0, 100.0]),
        [(0.0, 50), (0.1, 0)],
        "no extent to divide by"
    );
    assert!(arrange::is_fixed([(0.0, 50), (0.0, 10)]));
    assert!(!arrange::is_fixed([(1.0, 0), (0.0, 10)]));
}

#[test]
fn a_fit_centres_the_screen_and_never_blows_it_up() {
    let big = View::fit([1000.0, 600.0], [1920.0, 1080.0], 20.0);
    assert_eq!(big.zoom, 0.5, "width-bound: 960 of room for 1920");
    assert!(close(big.to_view([960.0, 540.0]), [500.0, 300.0]));

    let small = View::fit([1000.0, 900.0], [390.0, 844.0], 20.0);
    assert_eq!(small.zoom, 1.0);
}

#[test]
fn zooming_keeps_the_point_under_the_pointer() {
    let view = View {
        zoom: 0.5,
        pan: [10.0, 20.0],
    };
    let at = [110.0, 70.0];
    let under = view.to_canvas(at);
    let zoomed = view.zoomed(2.0, at);
    assert_eq!(zoomed.zoom, 1.0);
    assert!(close(zoomed.to_canvas(at), under));
}
