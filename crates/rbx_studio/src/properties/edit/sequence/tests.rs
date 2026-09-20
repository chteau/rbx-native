use super::*;

fn number(keypoints: &[(f32, f32, f32)]) -> NumberSequence {
    NumberSequence {
        keypoints: keypoints
            .iter()
            .map(|&(time, value, envelope)| NumberSequenceKeypoint {
                time,
                value,
                envelope,
            })
            .collect(),
    }
}

fn color(keypoints: &[(f32, [f32; 3], f32)]) -> ColorSequence {
    ColorSequence {
        keypoints: keypoints
            .iter()
            .map(|&(time, [r, g, b], envelope)| ColorSequenceKeypoint {
                time,
                color: Color3Data { r, g, b },
                envelope,
            })
            .collect(),
    }
}

/// The one property every `edit_text`/`parse` pair in this module owes the
/// panel: what a row shows is what the row accepts back.
#[test]
fn a_number_sequence_round_trips_through_its_text() {
    let sequence = number(&[(0.0, 1.0, 0.0), (0.5, 0.5, 0.25), (1.0, 0.0, 0.0)]);
    let text = number_sequence_text(&sequence);

    assert_eq!(text, "0, 1, 0; 0.5, 0.5, 0.25; 1, 0, 0");
    assert_eq!(parse_number_sequence(&text), Ok(sequence));
}

#[test]
fn a_colour_sequence_round_trips_through_its_text() {
    let sequence = color(&[(0.0, [1.0, 0.0, 0.0], 0.0), (1.0, [0.0, 0.0, 1.0], 0.0)]);
    let text = color_sequence_text(&sequence);

    assert_eq!(text, "0, 255, 0, 0; 1, 0, 0, 255");
    assert_eq!(parse_color_sequence(&sequence, &text), Ok(sequence));
}

#[test]
fn a_number_keypoint_may_leave_its_envelope_out() {
    assert_eq!(
        parse_number_sequence("0, 1; 1, 0"),
        Ok(number(&[(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)]))
    );
}

/// The text has no envelope column (the engine ignores the field), so an
/// edit must not be how a file loses one some other tool wrote.
#[test]
fn a_colour_keypoint_keeps_the_envelope_its_text_cannot_carry() {
    let current = color(&[(0.0, [1.0, 1.0, 1.0], 0.5), (1.0, [1.0, 1.0, 1.0], 0.25)]);

    let edited =
        parse_color_sequence(&current, "0, 0, 0, 0; 1, 255, 255, 255").expect("a legal sequence");

    let envelopes: Vec<f32> = edited.keypoints.iter().map(|k| k.envelope).collect();
    assert_eq!(envelopes, vec![0.5, 0.25]);
}

/// A keypoint the sequence did not have before has no envelope to inherit.
#[test]
fn a_new_colour_keypoint_starts_with_no_envelope() {
    let current = color(&[(0.0, [1.0, 1.0, 1.0], 0.5), (1.0, [1.0, 1.0, 1.0], 0.5)]);

    let edited =
        parse_color_sequence(&current, "0, 0, 0, 0; 0.5, 0, 0, 0; 1, 0, 0, 0").expect("legal");

    assert_eq!(edited.keypoints[2].envelope, 0.0);
}

#[test]
fn colour_channels_may_be_typed_as_zero_to_one() {
    let parsed = parse_color_sequence(&color(&[]), "0, 1, 0.5, 0; 1, 0, 0, 0").expect("legal");

    assert_eq!(parsed.keypoints[0].color.r, 1.0);
    assert_eq!(parsed.keypoints[0].color.g, 0.5);
}

/// One scale for the field, not one per keypoint — `1` beside a `255` is a
/// full channel on the 0-255 scale, not a white one on the 0-1 scale.
#[test]
fn one_channel_scale_is_picked_for_the_whole_sequence() {
    let parsed = parse_color_sequence(&color(&[]), "0, 1, 1, 1; 1, 255, 0, 0").expect("legal");

    assert_eq!(parsed.keypoints[0].color.r, 1.0 / 255.0);
    assert_eq!(parsed.keypoints[1].color.r, 1.0);
}

#[test]
fn a_trailing_separator_is_not_an_empty_keypoint() {
    assert_eq!(
        parse_number_sequence("0, 1, 0; 1, 0, 0;"),
        Ok(number(&[(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)]))
    );
}

/// The same bracket tolerance every other composite value in this panel has
/// (see `super::parse_numbers`), so a pasted `(0, 1, 0)` reads.
#[test]
fn brackets_inside_a_keypoint_are_cosmetic() {
    assert_eq!(
        parse_number_sequence("(0, 1, 0); (1, 0, 0)"),
        Ok(number(&[(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)]))
    );
}

#[test]
fn a_sequence_needs_at_least_two_keypoints() {
    assert!(parse_number_sequence("0, 1, 0").is_err());
    assert!(parse_color_sequence(&color(&[]), "0, 255, 255, 255").is_err());
}

/// `count` keypoints spread evenly from time 0 to time 1, so the only rule
/// under test is the count itself.
fn ramp(count: usize) -> String {
    (0..count)
        .map(|index| format!("{}, 0, 0", index as f32 / (count - 1) as f32))
        .collect::<Vec<String>>()
        .join("; ")
}

#[test]
fn a_sequence_is_capped_at_twenty_keypoints() {
    assert!(parse_number_sequence(&ramp(MAX_KEYPOINTS)).is_ok());
    assert!(parse_number_sequence(&ramp(MAX_KEYPOINTS + 1)).is_err());
}

/// `eval_number`/`eval_color` walk the keypoints in order and never reach one
/// that goes backwards, so this is a correctness rule rather than a tidiness
/// one.
#[test]
fn keypoint_times_may_not_go_backwards() {
    assert!(parse_number_sequence("0, 0, 0; 0.7, 1, 0; 0.3, 1, 0; 1, 0, 0").is_err());
}

/// Two keypoints at one time are a hard step, which Studio allows — see
/// `rbx_viewer`'s `two_keypoints_at_the_same_time_step`.
#[test]
fn two_keypoints_at_the_same_time_are_allowed() {
    assert!(parse_number_sequence("0, 0, 0; 0.5, 0, 0; 0.5, 1, 0; 1, 1, 0").is_ok());
}

#[test]
fn a_sequence_must_span_time_zero_to_one() {
    assert!(parse_number_sequence("0.2, 1, 0; 1, 0, 0").is_err());
    assert!(parse_number_sequence("0, 1, 0; 0.8, 0, 0").is_err());
}

#[test]
fn a_keypoint_with_the_wrong_number_of_terms_is_refused() {
    assert!(parse_number_sequence("0, 1, 0, 0; 1, 0, 0, 0").is_err());
    assert!(parse_color_sequence(&color(&[]), "0, 255, 255; 1, 0, 0").is_err());
    assert!(parse_number_sequence("0, nope, 0; 1, 0, 0").is_err());
}
