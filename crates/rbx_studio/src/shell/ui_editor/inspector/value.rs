//! A value as its field shows it and as it goes back: the numbers
//! `properties::edit` spells it in, and the percentage, hex or plain number
//! a designer reads and types.

use rbx_dom::Variant;

use super::spec::{Form, Spec};
use crate::properties::edit::edit_text;

/// A value's numbers, as `edit_text` spells them (a `Color3` in 0-255).
pub(super) fn numbers_of(value: &Variant) -> Option<Vec<f32>> {
    edit_text(value)?
        .split(',')
        .map(|number| number.trim().parse().ok())
        .collect()
}

/// The field's reading of a value whose numbers are `numbers`.
pub(super) fn read(form: Form, part: usize, numbers: &[f32]) -> Option<Vec<f32>> {
    match form {
        Form::Number { .. } => Some(vec![*numbers.get(part)?]),
        Form::Percent => Some(vec![((1.0 - numbers.get(part)?) * 100.0).round()]),
        Form::Hex => numbers.get(..3).map(<[f32]>::to_vec),
    }
}

/// The commit text for `numbers` with the field's `value` put in its parts.
pub(super) fn write(spec: Spec, mut numbers: Vec<f32>, value: &[f32]) -> String {
    let Some(&first) = value.first() else {
        return String::new();
    };
    if spec.form == Form::Hex {
        // As fractions: `properties::edit` reads a colour with no channel
        // over 1 as 0-1, which would turn #010000 into pure red.
        return value
            .iter()
            .map(|channel| (channel / 255.0).to_string())
            .collect::<Vec<_>>()
            .join(", ");
    }
    let stored = match spec.form {
        Form::Percent => 1.0 - first.clamp(0.0, 100.0) / 100.0,
        _ => first,
    };
    for &part in spec.parts {
        if let Some(number) = numbers.get_mut(part) {
            *number = stored;
        }
    }
    numbers
        .iter()
        .map(|number| number.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A reading as its field's text.
pub(super) fn show(form: Form, value: &[f32]) -> String {
    match form {
        Form::Hex => value
            .iter()
            .map(|channel| format!("{:02X}", channel.round().clamp(0.0, 255.0) as u8))
            .collect(),
        _ => value.first().map_or_else(String::new, |&number| {
            let text = format!("{number:.4}");
            let text = text.trim_end_matches('0').trim_end_matches('.');
            match text {
                "" | "-" | "-0" => "0".to_owned(),
                text => text.to_owned(),
            }
        }),
    }
}

/// A field's typed text as a value, or `None` where it is not one — a hex
/// colour with or without its `#`, a number with or without its unit.
pub(super) fn parse(form: Form, text: &str) -> Option<Vec<f32>> {
    let text = text.trim();
    match form {
        Form::Hex => {
            let digits = text.trim_start_matches('#');
            let digits = match digits.len() {
                3 => digits.chars().flat_map(|c| [c, c]).collect::<String>(),
                6 => digits.to_owned(),
                _ => return None,
            };
            (0..3)
                .map(|i| {
                    u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16)
                        .ok()
                        .map(f32::from)
                })
                .collect()
        }
        _ => text
            .trim_end_matches(['%', '°'])
            .trim_end_matches("px")
            .trim()
            .parse()
            .ok()
            .map(|number| vec![number]),
    }
}
