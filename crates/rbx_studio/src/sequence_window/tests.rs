use rbx_dom::{NumberSequence, NumberSequenceKeypoint, Variant};

// Named rather than a glob: `super` re-exports `gpui_kit::*`, whose own
// `test` attribute would shadow the one these tests mean.
use super::{field_text, sequence_fields, Editor, Field, Kind};

fn editor() -> Editor {
    Editor::open(&Variant::NumberSequence(NumberSequence {
        keypoints: vec![
            NumberSequenceKeypoint {
                time: 0.0,
                value: 0.25,
                envelope: 0.0,
            },
            NumberSequenceKeypoint {
                time: 1.0,
                value: 1.0,
                envelope: 0.5,
            },
        ],
    }))
    .expect("a sequence")
}

/// A `ColorSequenceKeypoint`'s envelope has no effect in Roblox's engine, so
/// the footer must not offer a field that writes one.
#[test]
fn a_colour_stop_has_a_time_and_nothing_else_to_type() {
    assert_eq!(
        sequence_fields(Kind::Color)
            .iter()
            .map(|(_, label)| *label)
            .collect::<Vec<_>>(),
        vec!["Time"]
    );
    assert_eq!(sequence_fields(Kind::Number).len(), 3);
}

#[test]
fn the_footer_reads_the_selected_stop() {
    let mut editor = editor();

    assert_eq!(field_text(&editor, Field::Time), "0");
    assert_eq!(field_text(&editor, Field::Value), "0.25");

    editor.selected = 1;
    assert_eq!(field_text(&editor, Field::Time), "1");
    assert_eq!(field_text(&editor, Field::Envelope), "0.5");
}
