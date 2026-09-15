//! Encoders for the whitespace-separated float-list types: NumberSequence,
//! ColorSequence, and NumberRange. Mirrors `value::sequence`.

use rbx_dom::{ColorSequence, NumberRange, NumberSequence};

use crate::serializer::writer::Writer;

pub(crate) fn number_range(writer: &mut Writer, name: &str, value: &NumberRange) {
    let text = format!("{} {}", value.min, value.max);
    writer.leaf("NumberRange", &[("name", name)], &text);
}

pub(crate) fn number_sequence(writer: &mut Writer, name: &str, value: &NumberSequence) {
    let mut text = String::new();
    for kp in &value.keypoints {
        text.push_str(&format!("{} {} {} ", kp.time, kp.value, kp.envelope));
    }
    writer.leaf("NumberSequence", &[("name", name)], &text);
}

pub(crate) fn color_sequence(writer: &mut Writer, name: &str, value: &ColorSequence) {
    let mut text = String::new();
    for kp in &value.keypoints {
        text.push_str(&format!(
            "{} {} {} {} {} ",
            kp.time, kp.color.r, kp.color.g, kp.color.b, kp.envelope
        ));
    }
    writer.leaf("ColorSequence", &[("name", name)], &text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::NumberSequenceKeypoint;

    #[test]
    fn number_sequence_writes_flat_float_triples() {
        let mut writer = Writer::new();
        number_sequence(
            &mut writer,
            "X",
            &NumberSequence {
                keypoints: vec![NumberSequenceKeypoint {
                    time: 0.0,
                    value: 6.0,
                    envelope: 3.0,
                }],
            },
        );
        assert_eq!(
            writer.into_string(),
            "<NumberSequence name=\"X\">0 6 3 </NumberSequence>\n"
        );
    }
}
