//! `NumberSequence`/`ColorSequence` as typed text: keypoints separated by
//! `;`, the numbers inside one keypoint by `,` — the same comma-joined
//! spelling every other composite value in this panel uses, one level deeper.
//!
//! A one-line field is not the curve/gradient widget these two eventually
//! want, and it is not meant to be: what it buys is that both types edit
//! through the panel's *existing* per-type path, which is also the only path
//! an attribute's value is allowed to take (see
//! `properties::attributes::edit_kind`) — so a sequence becomes creatable as
//! an attribute without a second set of editors existing anywhere.

use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint,
};

/// Roblox's own bounds on either sequence's keypoint list: `creator-docs`'
/// `ColorSequence.new` says "at least 2 and no more than 20 keypoints", and
/// `NumberSequence.new`'s parameter says the same with "a maximum of 20".
const MIN_KEYPOINTS: usize = 2;
const MAX_KEYPOINTS: usize = 20;

/// `time, value, envelope` per keypoint. The envelope is written out even
/// though it is almost always zero, so the text round-trips a file that does
/// use it rather than quietly flattening it on the next commit.
pub(super) fn number_sequence_text(sequence: &NumberSequence) -> String {
    join(&sequence.keypoints, |k| {
        format!("{}, {}, {}", k.time, k.value, k.envelope)
    })
}

/// `time, r, g, b` per keypoint, the colour as the 0-255 bytes every other
/// colour in this panel is typed in (see `properties::color3`). A
/// `ColorSequenceKeypoint`'s own envelope is deliberately not in the text:
/// Roblox's engine ignores it entirely, so a field for it would be three
/// characters of noise per keypoint — [`parse_color_sequence`] carries the
/// stored one through instead.
pub(super) fn color_sequence_text(sequence: &ColorSequence) -> String {
    join(&sequence.keypoints, |k| {
        format!(
            "{}, {}, {}, {}",
            k.time,
            super::channel(k.color.r),
            super::channel(k.color.g),
            super::channel(k.color.b)
        )
    })
}

pub(super) fn parse_number_sequence(text: &str) -> Result<NumberSequence, String> {
    let keypoints: Vec<NumberSequenceKeypoint> = keypoint_numbers(text)?
        .iter()
        .map(|numbers| match numbers[..] {
            // The envelope is optional on the way in the way it is optional
            // in `NumberSequenceKeypoint.new(time, value)`, so a sequence
            // that never uses one is half as much to type.
            [time, value] => Ok(NumberSequenceKeypoint {
                time,
                value,
                envelope: 0.0,
            }),
            [time, value, envelope] => Ok(NumberSequenceKeypoint {
                time,
                value,
                envelope,
            }),
            _ => Err(
                "each keypoint is \"time, value\" or \"time, value, envelope\", separated by ';'"
                    .to_owned(),
            ),
        })
        .collect::<Result<_, String>>()?;
    validate(&times(&keypoints, |k| k.time))?;
    Ok(NumberSequence { keypoints })
}

/// `current` is only read for the envelopes the text does not carry (see
/// [`color_sequence_text`]): each keypoint keeps whatever the keypoint at
/// its own index held, and a keypoint the sequence did not have before
/// starts at zero. Positional like this because the engine ignores the field
/// anyway — the point is only that editing a colour does not silently
/// rewrite a value some other tool wrote.
pub(super) fn parse_color_sequence(
    current: &ColorSequence,
    text: &str,
) -> Result<ColorSequence, String> {
    let rows = keypoint_numbers(text)?;
    // One scale for the whole field, rather than `parse_color3`'s per-triple
    // guess: keypoints of one gradient typed in two different scales is a
    // reading of the text nobody means, and picking it per keypoint would
    // make `0, 1, 1, 1; 1, 255, 0, 0` two of them.
    let is_255_scale = rows
        .iter()
        .flat_map(|numbers| numbers.iter().skip(1))
        .any(|channel| *channel > 1.0);
    let channel = |value: f32| {
        let value = if is_255_scale { value / 255.0 } else { value };
        value.clamp(0.0, 1.0)
    };

    let keypoints: Vec<ColorSequenceKeypoint> = rows
        .iter()
        .enumerate()
        .map(|(index, numbers)| match numbers[..] {
            [time, r, g, b] => Ok(ColorSequenceKeypoint {
                time,
                color: Color3Data {
                    r: channel(r),
                    g: channel(g),
                    b: channel(b),
                },
                envelope: current
                    .keypoints
                    .get(index)
                    .map_or(0.0, |keypoint| keypoint.envelope),
            }),
            _ => Err("each keypoint is \"time, r, g, b\", separated by ';'".to_owned()),
        })
        .collect::<Result<_, String>>()?;
    validate(&times(&keypoints, |k| k.time))?;
    Ok(ColorSequence { keypoints })
}

/// One list of numbers per `;`-separated keypoint, each split by
/// [`super::parse_numbers`] so the brackets and braces that spelling
/// tolerates are tolerated inside a keypoint too. A trailing `;` is ignored
/// rather than read as an empty keypoint — it is what someone typing the
/// next one leaves behind, not a value.
fn keypoint_numbers(text: &str) -> Result<Vec<Vec<f32>>, String> {
    text.split(';')
        .map(str::trim)
        .filter(|keypoint| !keypoint.is_empty())
        .map(|keypoint| super::parse_numbers(keypoint, super::count_numbers(keypoint)))
        .collect()
}

/// Roblox's own rules for either sequence (`creator-docs`,
/// `NumberSequence.new`/`ColorSequence.new`): [`MIN_KEYPOINTS`] to
/// [`MAX_KEYPOINTS`] of them, in non-descending time order, the first at time
/// 0 and the last at time 1. Checked here rather than left to whatever reads
/// the value later because `rbx_viewer`'s `eval_number`/`eval_color` walk the
/// list assuming exactly that — a keypoint whose time goes backwards is not
/// drawn slightly wrong, it is never reached at all.
fn validate(times: &[f32]) -> Result<(), String> {
    if times.len() < MIN_KEYPOINTS || times.len() > MAX_KEYPOINTS {
        return Err(format!(
            "a sequence needs {MIN_KEYPOINTS} to {MAX_KEYPOINTS} keypoints, got {}",
            times.len()
        ));
    }
    if times.windows(2).any(|pair| pair[1] < pair[0]) {
        return Err("keypoint times must not go backwards".to_owned());
    }
    if times[0] != 0.0 || times[times.len() - 1] != 1.0 {
        return Err("the first keypoint must be at time 0 and the last at time 1".to_owned());
    }
    Ok(())
}

fn times<T>(keypoints: &[T], time: impl Fn(&T) -> f32) -> Vec<f32> {
    keypoints.iter().map(time).collect()
}

fn join<T>(keypoints: &[T], format_one: impl Fn(&T) -> String) -> String {
    keypoints
        .iter()
        .map(format_one)
        .collect::<Vec<String>>()
        .join("; ")
}

#[cfg(test)]
#[path = "sequence/tests.rs"]
mod tests;
