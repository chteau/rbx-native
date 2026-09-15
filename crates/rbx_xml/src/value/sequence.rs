//! Decoders for the whitespace-separated float-list types: NumberSequence,
//! ColorSequence, and NumberRange.

use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, NumberRange, NumberSequence,
    NumberSequenceKeypoint, Variant,
};

use super::scalar::parse_f32;

fn floats(text: &str) -> Vec<f32> {
    text.split_whitespace().map(parse_f32).collect()
}

pub(crate) fn number_range(text: &str) -> Variant {
    let values = floats(text);
    Variant::NumberRange(NumberRange {
        min: values.first().copied().unwrap_or(0.0),
        max: values.get(1).copied().unwrap_or(0.0),
    })
}

pub(crate) fn number_sequence(text: &str) -> Variant {
    let keypoints = floats(text)
        .as_chunks::<3>()
        .0
        .iter()
        .map(|chunk| NumberSequenceKeypoint {
            time: chunk[0],
            value: chunk[1],
            envelope: chunk[2],
        })
        .collect();
    Variant::NumberSequence(NumberSequence { keypoints })
}

pub(crate) fn color_sequence(text: &str) -> Variant {
    let keypoints = floats(text)
        .as_chunks::<5>()
        .0
        .iter()
        .map(|chunk| ColorSequenceKeypoint {
            time: chunk[0],
            color: Color3Data {
                r: chunk[1],
                g: chunk[2],
                b: chunk[3],
            },
            envelope: chunk[4],
        })
        .collect();
    Variant::ColorSequence(ColorSequence { keypoints })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_sequence_reads_the_spec_example() {
        let value = number_sequence("0 6 3 1 4 2 ");
        assert_eq!(
            value,
            Variant::NumberSequence(NumberSequence {
                keypoints: vec![
                    NumberSequenceKeypoint {
                        time: 0.0,
                        value: 6.0,
                        envelope: 3.0
                    },
                    NumberSequenceKeypoint {
                        time: 1.0,
                        value: 4.0,
                        envelope: 2.0
                    },
                ],
            })
        );
    }

    #[test]
    fn color_sequence_reads_the_spec_example() {
        let value =
            color_sequence("0 0.376471 0.25098 0.12549 0 1 0.0196078 0.0392157 0.0588235 0 ");
        assert_eq!(
            value,
            Variant::ColorSequence(ColorSequence {
                keypoints: vec![
                    ColorSequenceKeypoint {
                        time: 0.0,
                        color: Color3Data {
                            r: 0.376471,
                            g: 0.25098,
                            b: 0.12549
                        },
                        envelope: 0.0,
                    },
                    ColorSequenceKeypoint {
                        time: 1.0,
                        color: Color3Data {
                            r: 0.0196078,
                            g: 0.0392157,
                            b: 0.0588235
                        },
                        envelope: 0.0,
                    },
                ],
            })
        );
    }

    #[test]
    fn number_range_reads_min_and_max() {
        assert_eq!(
            number_range("0.15625 1337 "),
            Variant::NumberRange(NumberRange {
                min: 0.15625,
                max: 1337.0
            })
        );
    }
}
