use rbx_dom::{ColorSequenceKeypoint, NumberSequenceKeypoint};

use super::*;

fn number(points: &[(f32, f32, f32)]) -> Variant {
    Variant::NumberSequence(NumberSequence {
        keypoints: points
            .iter()
            .map(|&(time, value, envelope)| NumberSequenceKeypoint {
                time,
                value,
                envelope,
            })
            .collect(),
    })
}

fn color(points: &[(f32, [f32; 3])]) -> Variant {
    Variant::ColorSequence(ColorSequence {
        keypoints: points
            .iter()
            .map(|&(time, [r, g, b])| ColorSequenceKeypoint {
                time,
                color: Color3Data { r, g, b },
                envelope: 0.0,
            })
            .collect(),
    })
}

fn editor(value: &Variant) -> Editor {
    Editor::open(value).expect("a sequence")
}

#[test]
fn only_a_sequence_opens_an_editor() {
    assert!(Editor::open(&Variant::Bool(true)).is_none());
    assert!(Editor::open(&number(&[(0., 0., 0.)])).is_some());
}

/// The graph is an input to the text path, never a second write path, so
/// the text it produces has to be the text the parser reads back.
#[test]
fn the_text_a_graph_commits_is_the_text_the_parser_reads() {
    let mut editor = editor(&number(&[(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)]));
    editor.insert(0.5);

    let text = editor.text();
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let parsed = crate::properties::edit::parse(&editor.value(), &db, "", "", &text)
        .expect("the graph's own text");

    assert_eq!(parsed, editor.value());
}

#[test]
fn a_colour_graph_commits_colour_text() {
    let mut editor = editor(&color(&[(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])]));
    editor.insert(0.5);

    assert_eq!(
        editor.text(),
        "0, 255, 0, 0; 0.5, 128, 0, 128; 1, 0, 0, 255"
    );
}

/// Roblox's constructors refuse a sequence that does not start at 0 and end
/// at 1, so the two ends are not draggable in time at all.
#[test]
fn the_end_stops_keep_their_times() {
    let mut editor = editor(&number(&[(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)]));

    editor.drag = Some(Drag {
        index: 0,
        handle: Handle::Point,
    });
    editor.drag_to(0.4, 0.5);
    editor.drag = Some(Drag {
        index: 1,
        handle: Handle::Point,
    });
    editor.drag_to(0.6, 0.5);

    assert_eq!(editor.stops[0].time, 0.0);
    assert_eq!(editor.stops[1].time, 1.0);
    assert_eq!(editor.stops[0].value, 0.5);
}

/// A drag that overtakes a neighbour would reorder the list, which is the
/// one thing `eval_number` cannot survive.
#[test]
fn a_dragged_stop_cannot_pass_its_neighbours() {
    let mut editor = editor(&number(&[
        (0.0, 0.0, 0.0),
        (0.3, 1.0, 0.0),
        (0.7, 1.0, 0.0),
        (1.0, 0.0, 0.0),
    ]));

    editor.drag = Some(Drag {
        index: 1,
        handle: Handle::Point,
    });
    editor.drag_to(0.95, 0.5);
    assert_eq!(editor.stops[1].time, 0.7);

    editor.drag_to(-0.5, 0.5);
    assert_eq!(editor.stops[1].time, 0.0);
}

#[test]
fn dragging_the_envelope_handle_sets_a_symmetric_spread() {
    let mut editor = editor(&number(&[(0.0, 1.0, 0.0), (1.0, 1.0, 0.0)]));

    editor.drag = Some(Drag {
        index: 0,
        handle: Handle::Envelope,
    });
    editor.drag_to(0.0, 1.5);
    assert_eq!(editor.stops[0].envelope, 0.5);

    // Dragged below the stop, the band is still half a unit wide.
    editor.drag_to(0.0, 0.75);
    assert_eq!(editor.stops[0].envelope, 0.25);
}

#[test]
fn an_inserted_stop_lands_on_the_curve_it_split() {
    let mut editor = editor(&number(&[(0.0, 0.0, 0.0), (1.0, 4.0, 0.0)]));

    assert!(editor.insert(0.25));

    assert_eq!(editor.stops[1].time, 0.25);
    assert_eq!(editor.stops[1].value, 1.0);
    assert_eq!(editor.selected, 1);
}

#[test]
fn an_inserted_colour_stop_lands_on_the_ramp_it_split() {
    let mut editor = editor(&color(&[(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 1.0, 0.0])]));

    editor.insert(0.5);

    assert_eq!(editor.stops[1].color.r, 0.5);
    assert_eq!(editor.stops[1].color.g, 0.5);
}

#[test]
fn a_sequence_cannot_grow_past_roblox_ceiling() {
    let mut editor = editor(&number(&[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)]));
    for index in 1..MAX_STOPS - 1 {
        assert!(editor.insert(index as f32 / MAX_STOPS as f32), "{index}");
    }

    assert_eq!(editor.stops.len(), MAX_STOPS);
    assert!(!editor.insert(0.999));
}

#[test]
fn neither_end_can_be_deleted_and_neither_can_the_last_pair() {
    let mut editor = editor(&number(&[
        (0.0, 0.0, 0.0),
        (0.5, 1.0, 0.0),
        (1.0, 0.0, 0.0),
    ]));

    editor.selected = 0;
    assert!(!editor.remove_selected());
    editor.selected = 2;
    assert!(!editor.remove_selected());

    editor.selected = 1;
    assert!(editor.remove_selected());
    assert_eq!(editor.stops.len(), 2);
    assert!(!editor.remove_selected());
}

/// The ceiling is what a drag clamps against, so it has to cover the
/// envelope band too — otherwise the top of a band is unreachable.
#[test]
fn the_value_axis_fits_the_stops_and_their_envelopes() {
    // Never under 1, so a transparency curve is not squashed flat, and
    // always clear of the tallest stop by `HEADROOM`.
    for (points, ceiling) in [
        (vec![(0.0, 0.2, 0.0), (1.0, 0.7, 0.0)], 1.5),
        (vec![(0.0, 0.0, 0.0), (1.0, 3.0, 0.0)], 3.5),
        (vec![(0.0, 0.0, 2.0), (1.0, 3.0, 0.0)], 3.5),
        (vec![(0.0, 0.0, 4.0), (1.0, 3.0, 0.0)], 5.0),
    ] {
        let fitted = editor(&number(&points)).ceiling();
        assert_eq!(fitted, ceiling, "{points:?}");
        let tallest = points
            .iter()
            .map(|&(_, value, envelope)| value + envelope)
            .fold(0.0_f32, f32::max);
        assert!(fitted > tallest, "{points:?} reaches {tallest}");
    }
}

#[test]
fn a_click_grabs_the_nearest_handle_and_empty_plot_grabs_nothing() {
    let editor = editor(&number(&[
        (0.0, 0.0, 0.0),
        (0.5, 1.0, 0.25),
        (1.0, 0.0, 0.0),
    ]));

    assert_eq!(
        editor.grab(0.5, 1.0),
        Some(Drag {
            index: 1,
            handle: Handle::Point
        })
    );
    assert_eq!(
        editor.grab(0.5, 1.25),
        Some(Drag {
            index: 1,
            handle: Handle::Envelope
        })
    );
    assert_eq!(editor.grab(0.25, 0.6), None);
}

/// A typed number obeys the same clamps a drag does, so the two cannot
/// disagree about what is legal.
#[test]
fn typed_fields_clamp_the_way_a_drag_does() {
    let mut editor = editor(&number(&[
        (0.0, 0.0, 0.0),
        (0.5, 1.0, 0.0),
        (1.0, 0.0, 0.0),
    ]));
    editor.selected = 1;

    assert!(editor.set_field(Field::Time, "2"));
    assert_eq!(editor.stops[1].time, 1.0);
    assert!(editor.set_field(Field::Value, "-4"));
    assert_eq!(editor.stops[1].value, 0.0);
    assert!(!editor.set_field(Field::Value, "not a number"));

    editor.selected = 0;
    assert!(!editor.set_field(Field::Time, "0.5"));
    assert_eq!(editor.stops[0].time, 0.0);
}

#[test]
fn the_plot_rectangle_maps_both_ways() {
    let rect = Rect {
        x: 100.0,
        y: 50.0,
        width: 400.0,
        height: 200.0,
    };

    assert_eq!(rect.at(300.0, 150.0, 2.0), (0.5, 1.0));
    assert_eq!(rect.of(0.5, 1.0, 2.0), (300.0, 150.0));
    // Outside the plot, time clamps and the value floors at zero.
    assert_eq!(rect.at(0.0, 400.0, 2.0), (0.0, 0.0));
}

#[test]
fn sampling_matches_the_renderers_own_linear_rule() {
    let editor = editor(&number(&[(0.0, 0.0, 0.0), (1.0, 4.0, 0.0)]));

    assert_eq!(editor.sample(0.0).value, 0.0);
    assert_eq!(editor.sample(0.25).value, 1.0);
    assert_eq!(editor.sample(1.0).value, 4.0);
}

#[test]
fn two_stops_at_one_time_sample_as_a_hard_step() {
    let editor = editor(&color(&[
        (0.0, [1.0, 0.0, 0.0]),
        (0.5, [1.0, 0.0, 0.0]),
        (0.5, [0.0, 0.0, 1.0]),
        (1.0, [0.0, 0.0, 1.0]),
    ]));

    assert_eq!(editor.sample(0.49).color.r, 1.0);
    assert_eq!(editor.sample(0.51).color.b, 1.0);
}

#[test]
fn a_colour_stop_offers_no_envelope_handle() {
    let editor = editor(&color(&[(0.0, [1.0, 1.0, 1.0]), (1.0, [1.0, 1.0, 1.0])]));

    assert_eq!(
        editor.grab(0.0, 0.0),
        Some(Drag {
            index: 0,
            handle: Handle::Point
        })
    );
}

/// A marker under a ramp has no height to aim at, so a grab has to be
/// decided on time alone — otherwise a click halfway up the ramp misses
/// every stop and silently adds one instead.
#[test]
fn a_colour_stop_is_grabbed_by_time_alone() {
    let mut editor = editor(&color(&[
        (0.0, [1.0, 0.0, 0.0]),
        (0.5, [0.0, 1.0, 0.0]),
        (1.0, [0.0, 0.0, 1.0]),
    ]));

    assert_eq!(
        editor.grab(0.5, 0.9),
        Some(Drag {
            index: 1,
            handle: Handle::Point
        })
    );
    assert_eq!(editor.grab(0.25, 0.9), None);

    // …and dragging one leaves the unused value half alone.
    editor.drag = editor.grab(0.5, 0.9);
    editor.drag_to(0.7, 1.4);
    assert_eq!(editor.stops[1].time, 0.7);
    assert_eq!(editor.stops[1].value, 0.0);
}
