//! Turns one row's typed text back into a `Variant` shaped like the value it
//! replaces, and commits it to the DOM the same way the Command Bar does:
//! `WeakDom::set_property` (or `set_name` for the synthetic `Name` row, which
//! lives on `Instance` itself rather than in its property map).

use rbx_dom::{
    CFrameData, Color3Data, NumberRange, Ref, UDim, UDim2, Variant, Vector2Data, Vector3Data,
    WeakDom,
};
use rbx_reflection::ReflectionDatabase;

/// `Name` is not a key in `Instance::properties()` — the panel synthesizes a
/// row for it and [`commit`] routes it to `WeakDom::set_name` instead of
/// `set_property`.
pub(crate) const NAME_PROPERTY: &str = "Name";

/// The text an editable row's `Input` starts with: always round-trips through
/// [`parse`], which is why it can differ from the read-only column's
/// rbxdump-style text (that one must match the dump byte for byte; this one
/// only has to be typeable). `CFrame` in particular loses its rotation here —
/// there is no accepted syntax for it, so only the position is ever shown or
/// written back; the rotation is carried through unchanged by [`parse`].
///
/// `None` means the type is not one [`parse`] understands, which keeps the
/// row read-only.
pub(crate) fn edit_text(value: &Variant) -> Option<String> {
    match value {
        Variant::Bool(flag) => Some(flag.to_string()),
        // No BrickColor→RGB palette table is bundled here (Roblox's is ~140
        // entries and not derivable from anything else already parsed), so
        // the panel edits the raw palette index rather than a color swatch —
        // see `properties::EditKind::Text`'s doc comment.
        Variant::BrickColor(index) => Some(index.to_string()),
        Variant::Int32(number) => Some(number.to_string()),
        Variant::Int64(number) => Some(number.to_string()),
        Variant::Float32(number) => Some(number.to_string()),
        Variant::Float64(number) => Some(number.to_string()),
        Variant::String(text) => Some(text.clone()),
        Variant::Vector2(v) => Some(format!("{}, {}", v.x, v.y)),
        Variant::Vector3(v) => Some(format!("{}, {}, {}", v.x, v.y, v.z)),
        Variant::Color3(color) => Some(format!(
            "{}, {}, {}",
            channel(color.r),
            channel(color.g),
            channel(color.b)
        )),
        Variant::Color3uint8 { r, g, b } => Some(format!("{r}, {g}, {b}")),
        // The ordinal, not the item's name: resolving that needs the class
        // and a `ReflectionDatabase`, which this free function does not have.
        // Typing the name still works — see `parse_enum`.
        Variant::Enum(raw) => Some(raw.to_string()),
        Variant::UDim(u) => Some(format!("{}, {}", u.scale, u.offset)),
        Variant::UDim2(u) => Some(format!(
            "{}, {}, {}, {}",
            u.x.scale, u.x.offset, u.y.scale, u.y.offset
        )),
        Variant::CFrame(frame) => Some(format!(
            "{}, {}, {}",
            frame.position.x, frame.position.y, frame.position.z
        )),
        Variant::NumberRange(range) => Some(format!("{}, {}", range.min, range.max)),
        _ => None,
    }
}

/// Writes one row's edit into `dom`, taking the same latitude the Command Bar
/// does: the caller owns handing `dom` in and back out around this call. On
/// success, the value the write replaced (`Name`'s previous name, wrapped for
/// uniformity). On failure, `dom` is untouched — every mutation happens after
/// parsing succeeds.
pub(crate) fn commit(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    prop_name: &str,
    text: &str,
) -> Result<Option<Variant>, String> {
    if prop_name == NAME_PROPERTY {
        return dom
            .set_name(reference, text.trim())
            .map(|old| Some(Variant::String(old)))
            .map_err(|err| err.to_string());
    }

    let instance = dom
        .get(reference)
        .ok_or_else(|| "the instance no longer exists".to_string())?;
    let class = instance.class().to_owned();
    let current = instance
        .properties()
        .get(prop_name)
        .ok_or_else(|| format!("{prop_name} has no current value to type-check against"))?
        .clone();

    let value = parse(&current, db, &class, prop_name, text)?;
    dom.set_property(reference, prop_name, value)
        .map_err(|err| err.to_string())
}

/// Parses `text` into a value shaped like `current`. `class`/`prop_name` are
/// only needed to resolve an `Enum`'s item names through `db`.
fn parse(
    current: &Variant,
    db: &ReflectionDatabase,
    class: &str,
    prop_name: &str,
    text: &str,
) -> Result<Variant, String> {
    let text = text.trim();
    match current {
        Variant::Bool(_) => parse_bool(text).map(Variant::Bool),
        Variant::BrickColor(_) => text
            .parse::<u32>()
            .map(Variant::BrickColor)
            .map_err(|_| not_a_number(text)),
        Variant::Int32(_) => text
            .parse::<i32>()
            .map(Variant::Int32)
            .map_err(|_| not_a_number(text)),
        Variant::Int64(_) => text
            .parse::<i64>()
            .map(Variant::Int64)
            .map_err(|_| not_a_number(text)),
        Variant::Float32(_) => text
            .parse::<f32>()
            .map(Variant::Float32)
            .map_err(|_| not_a_number(text)),
        Variant::Float64(_) => text
            .parse::<f64>()
            .map(Variant::Float64)
            .map_err(|_| not_a_number(text)),
        Variant::String(_) => Ok(Variant::String(text.to_owned())),
        Variant::Vector2(_) => {
            let n = parse_numbers(text, 2)?;
            Ok(Variant::Vector2(Vector2Data { x: n[0], y: n[1] }))
        }
        Variant::Vector3(_) => {
            let n = parse_numbers(text, 3)?;
            Ok(Variant::Vector3(Vector3Data {
                x: n[0],
                y: n[1],
                z: n[2],
            }))
        }
        Variant::Color3(_) => parse_color3(text).map(Variant::Color3),
        Variant::Color3uint8 { .. } => {
            let (r, g, b) = parse_color3uint8(text)?;
            Ok(Variant::Color3uint8 { r, g, b })
        }
        Variant::Enum(_) => parse_enum(db, class, prop_name, text),
        Variant::UDim(_) => parse_udim(text).map(Variant::UDim),
        Variant::UDim2(_) => parse_udim2(text).map(Variant::UDim2),
        // Position only; the rotation matrix has no accepted syntax here, so
        // whatever `current` was carrying is kept as-is (see `edit_text`).
        Variant::CFrame(frame) => {
            let n = parse_numbers(text, 3)?;
            Ok(Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: n[0],
                    y: n[1],
                    z: n[2],
                },
                rotation: frame.rotation,
            }))
        }
        Variant::NumberRange(_) => {
            let n = parse_numbers(text, 2)?;
            Ok(Variant::NumberRange(NumberRange {
                min: n[0],
                max: n[1],
            }))
        }
        other => Err(format!("{} is read-only", type_name(other))),
    }
}

fn not_a_number(text: &str) -> String {
    format!("{text:?} is not a number")
}

fn parse_bool(text: &str) -> Result<bool, String> {
    match text.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{text:?} is not true or false")),
    }
}

/// Splits `text` into exactly `count` numbers. Brackets and braces are
/// stripped first so `(1, 2, 3)`, `{1, 2}` and a bare `1, 2, 3` all read the
/// same — the punctuation in [`edit_text`]'s output and in this module's doc
/// comment is a hint for the reader, not syntax the parser requires.
fn parse_numbers(text: &str, count: usize) -> Result<Vec<f32>, String> {
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, '(' | ')' | '{' | '}' | '[' | ']'))
        .collect();
    let parts: Vec<&str> = cleaned.split(',').map(str::trim).collect();
    if parts.len() != count {
        return Err(format!(
            "expected {count} comma-separated numbers, got {}",
            parts.len()
        ));
    }
    parts
        .iter()
        .map(|part| part.parse::<f32>().map_err(|_| not_a_number(part)))
        .collect()
}

/// 0–255 like Studio's own display, or 0–1 floats: a value over 1 in any
/// channel settles it as the 0–255 scale, since a 0–1 color can never reach
/// that high.
fn parse_color3(text: &str) -> Result<Color3Data, String> {
    let n = parse_numbers(text, 3)?;
    let is_255_scale = n.iter().any(|value| *value > 1.0);
    let channel = |value: f32| {
        let value = if is_255_scale { value / 255.0 } else { value };
        value.clamp(0.0, 1.0)
    };
    Ok(Color3Data {
        r: channel(n[0]),
        g: channel(n[1]),
        b: channel(n[2]),
    })
}

fn parse_color3uint8(text: &str) -> Result<(u8, u8, u8), String> {
    let n = parse_numbers(text, 3)?;
    let channel = |value: f32| value.round().clamp(0.0, 255.0) as u8;
    Ok((channel(n[0]), channel(n[1]), channel(n[2])))
}

fn parse_udim(text: &str) -> Result<UDim, String> {
    let n = parse_numbers(text, 2)?;
    Ok(UDim {
        scale: n[0],
        offset: n[1].round() as i32,
    })
}

/// `{sx, ox}, {sy, oy}`: the braces are cosmetic (see `parse_numbers`), so
/// this is really just 4 numbers in `x`-then-`y`, `scale`-then-`offset` order.
fn parse_udim2(text: &str) -> Result<UDim2, String> {
    let n = parse_numbers(text, 4)?;
    Ok(UDim2 {
        x: UDim {
            scale: n[0],
            offset: n[1].round() as i32,
        },
        y: UDim {
            scale: n[2],
            offset: n[3].round() as i32,
        },
    })
}

/// The item's name, case-insensitive, or its raw ordinal.
fn parse_enum(
    db: &ReflectionDatabase,
    class: &str,
    prop_name: &str,
    text: &str,
) -> Result<Variant, String> {
    let property = db
        .resolve_property(class, prop_name)
        .ok_or_else(|| format!("no reflection data for {class}.{prop_name}"))?;
    let enum_name = &property.value_type;
    let items = db
        .enum_items(enum_name)
        .ok_or_else(|| format!("{enum_name} is not a known enum"))?;

    if let Ok(number) = text.parse::<u32>() {
        return Ok(Variant::Enum(number));
    }
    items
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(text))
        .map(|(_, value)| Variant::Enum(*value))
        .ok_or_else(|| format!("{text:?} is not a member of {enum_name}"))
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Only reached when a row's type was not one [`edit_text`] approved for
/// editing in the first place; names the type so a defensive caller (or a
/// test poking `parse` directly) gets a legible error instead of a panic.
fn type_name(value: &Variant) -> &'static str {
    match value {
        Variant::Vector3int16 { .. } => "Vector3int16",
        Variant::Ray { .. } => "Ray",
        Variant::Faces(_) => "Faces",
        Variant::Axes(_) => "Axes",
        Variant::OptionalCFrame(_) => "OptionalCFrame",
        Variant::Ref(_) => "Ref",
        Variant::NumberSequence(_) => "NumberSequence",
        Variant::ColorSequence(_) => "ColorSequence",
        Variant::Rect(_) => "Rect",
        Variant::PhysicalProperties(_) => "PhysicalProperties",
        Variant::SharedString(_) => "SharedString",
        Variant::UniqueId(_) => "UniqueId",
        Variant::Font(_) => "Font",
        Variant::SecurityCapabilities(_) => "SecurityCapabilities",
        Variant::Content(_) => "Content",
        Variant::Unknown { .. } => "Unknown",
        _ => "this type",
    }
}

#[cfg(test)]
mod tests;
