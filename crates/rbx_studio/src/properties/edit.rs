//! Turns one row's typed text back into a `Variant` shaped like the value it
//! replaces, and commits it to the DOM the same way the Command Bar does:
//! `WeakDom::set_property` (or `set_name` for the synthetic `Name` row, which
//! lives on `Instance` itself rather than in its property map).

use rbx_dom::{
    Axes, CFrameData, Color3Data, Faces, NumberRange, PhysicalProperties, Rect, Ref, UDim, UDim2,
    Variant, Vector2Data, Vector3Data, WeakDom,
};
use rbx_reflection::ReflectionDatabase;

mod font;
mod sequence;

use font::{font_text, parse_font, synced};
use sequence::{
    color_sequence_text, number_sequence_text, parse_color_sequence, parse_number_sequence,
};

/// `Name` is not a key in `Instance::properties()` — the panel synthesizes a
/// row for it and [`commit`] routes it to `WeakDom::set_name` instead of
/// `set_property`.
pub(crate) const NAME_PROPERTY: &str = "Name";

/// Not a real Roblox `Folder` property at all — see `crate::folder_colors`.
/// `Properties::rows` synthesizes this row only for a `Folder`, and
/// `shell::folder_color::commit_folder_color` routes its commit to that
/// module's local store instead of reaching this file's [`commit`] at all;
/// spelled distinctly (capitalized, spaced) so nobody mistakes it for a real
/// dump property.
pub(crate) const FOLDER_COLOR_PROPERTY: &str = "Explorer Colour";

/// The text an editable row's `Input` starts with: always round-trips through
/// [`parse`], which is why it can differ from the read-only column's
/// rbxdump-style text (that one must match the dump byte for byte; this one
/// only has to be typeable). `CFrame` in particular shows only its position
/// here — a nine-term rotation matrix in a one-line field is not something
/// anyone edits by hand — and [`parse`] carries the rotation through unchanged
/// when it is given three numbers, so the row still round-trips. The longer
/// forms it also accepts (see [`parse_cframe`]) are what the viewport's Rotate
/// drag writes through.
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
        Variant::Vector3int16 { x, y, z } => Some(format!("{x}, {y}, {z}")),
        // Flags commit as their own booleans, in the fixed order
        // `properties`' `FACES`/`AXES` name them.
        Variant::Faces(f) => Some(format!(
            "{}, {}, {}, {}, {}, {}",
            f.right, f.top, f.back, f.left, f.bottom, f.front
        )),
        Variant::Axes(a) => Some(format!("{}, {}, {}", a.x, a.y, a.z)),
        Variant::Ray { origin, direction } => Some(format!(
            "{}, {}, {}, {}, {}, {}",
            origin.x, origin.y, origin.z, direction.x, direction.y, direction.z
        )),
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
        Variant::CFrame(frame) => Some(cframe_text(frame)),
        // An absent one seeds from [`IDENTITY_CFRAME`] rather than staying
        // read-only: its fields are hidden until the row's present/absent
        // checkbox turns the value on, and that is the value it turns on to.
        Variant::OptionalCFrame(frame) => Some(cframe_text(&frame.unwrap_or(IDENTITY_CFRAME))),
        Variant::NumberRange(range) => Some(format!("{}, {}", range.min, range.max)),
        Variant::Rect(rect) => Some(format!(
            "{}, {}, {}, {}",
            rect.min.x, rect.min.y, rect.max.x, rect.max.y
        )),
        Variant::Font(font) => Some(font_text(font)),
        // Keypoints along a `;`, each one's numbers along a `,`. Nobody
        // types this — `shell::sequence_panel`'s graph is the editor; see
        // `sequence` for why its commits still come through here.
        Variant::NumberSequence(sequence) => Some(number_sequence_text(sequence)),
        Variant::ColorSequence(sequence) => Some(color_sequence_text(sequence)),
        // A `Default` seeds from [`DEFAULT_PHYSICAL`] for the same reason an
        // absent `OptionalCFrame` seeds from the identity: the five fields
        // are hidden until the row's Custom box turns them on, and that is
        // what they turn on to.
        Variant::PhysicalProperties(physical) => Some(physical_text(physical)),
        _ => None,
    }
}

/// What a `PhysicalProperties::Default` becomes the moment the row's Custom
/// box is ticked: Roblox's own defaults for `Plastic`, the material every
/// newly inserted `Part` carries — density `0.7`, friction `0.3`, elasticity
/// `0.5`, and both weights `1`. There is no better answer available here,
/// because `Default` means "derive these from the material" and this
/// function is not given the part.
pub(crate) const DEFAULT_PHYSICAL: PhysicalProperties = PhysicalProperties::Custom {
    density: 0.7,
    friction: 0.3,
    elasticity: 0.5,
    friction_weight: 1.0,
    elasticity_weight: 1.0,
};

/// The five numbers, comma-joined the way [`parse`] reads them back. A
/// `Default` lends its fields from [`DEFAULT_PHYSICAL`] rather than showing
/// blanks, so the editor under an unticked box is never empty.
fn physical_text(physical: &PhysicalProperties) -> String {
    let PhysicalProperties::Custom {
        density,
        friction,
        elasticity,
        friction_weight,
        elasticity_weight,
    } = custom_or_default(physical)
    else {
        unreachable!("custom_or_default never returns Default");
    };
    format!("{density}, {friction}, {elasticity}, {friction_weight}, {elasticity_weight}")
}

fn custom_or_default(physical: &PhysicalProperties) -> PhysicalProperties {
    match physical {
        PhysicalProperties::Default => DEFAULT_PHYSICAL,
        custom => *custom,
    }
}

/// The Custom box, then the five fields — the same two-shaped commit an
/// `OptionalCFrame` row makes (see [`parse_optional_cframe`]). Unticking
/// returns the value to `Default` rather than zeroing the numbers: the two
/// are different physics, not the same physics written differently.
fn parse_physical(current: &PhysicalProperties, text: &str) -> Result<Variant, String> {
    match text.trim() {
        "false" => Ok(Variant::PhysicalProperties(PhysicalProperties::Default)),
        "true" => Ok(Variant::PhysicalProperties(custom_or_default(current))),
        _ => {
            let n = parse_numbers(text, 5)?;
            Ok(Variant::PhysicalProperties(PhysicalProperties::Custom {
                density: n[0],
                friction: n[1],
                elasticity: n[2],
                friction_weight: n[3],
                elasticity_weight: n[4],
            }))
        }
    }
}

mod orientation;

/// What an absent `OptionalCFrame` becomes the moment its row says it has a
/// value: there is no orientation to preserve, so it starts unrotated at the
/// origin.
const IDENTITY_CFRAME: CFrameData = CFrameData {
    position: Vector3Data {
        x: 0.,
        y: 0.,
        z: 0.,
    },
    rotation: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
};

/// An `OptionalCFrame` row has two controls committing through this one
/// textual path: a present/absent checkbox, which commits `true`/`false` (the
/// same one-flag text `EditKind::Flags` produces), and the `CFrame` fields
/// under it, which commit six numbers. Neither spelling can be mistaken for
/// the other, so the text alone says which one moved.
///
/// Turning the checkbox on keeps whatever the value already held, so a
/// clear-then-restore round-trips instead of silently resetting to the origin.
fn parse_optional_cframe(current: &Option<CFrameData>, text: &str) -> Result<Variant, String> {
    match text.trim() {
        "false" => Ok(Variant::OptionalCFrame(None)),
        "true" => Ok(Variant::OptionalCFrame(Some(
            current.unwrap_or(IDENTITY_CFRAME),
        ))),
        _ => parse_cframe(&current.unwrap_or(IDENTITY_CFRAME), text)
            .map(|frame| Variant::OptionalCFrame(Some(frame))),
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
    // A saved place carries both `Font` and `FontFace`, and the viewer draws
    // the face; the enum edit would otherwise change nothing on screen.
    let partner =
        synced(prop_name, &value).filter(|(name, _)| instance.properties().contains_key(*name));
    let previous = dom
        .set_property(reference, prop_name, value)
        .map_err(|err| err.to_string())?;
    if let Some((name, value)) = partner {
        dom.set_property(reference, name, value)
            .map_err(|err| err.to_string())?;
    }
    Ok(previous)
}

/// Parses `text` into a value shaped like `current`. `class`/`prop_name` are
/// only needed to resolve an `Enum`'s item names through `db`.
pub(crate) fn parse(
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
        Variant::Faces(_) => {
            let f = parse_flags(text, 6)?;
            Ok(Variant::Faces(Faces {
                right: f[0],
                top: f[1],
                back: f[2],
                left: f[3],
                bottom: f[4],
                front: f[5],
            }))
        }
        Variant::Axes(_) => {
            let a = parse_flags(text, 3)?;
            Ok(Variant::Axes(Axes {
                x: a[0],
                y: a[1],
                z: a[2],
            }))
        }
        Variant::Vector3int16 { .. } => {
            let n = parse_numbers(text, 3)?;
            // Rounded and clamped, not `as i16`. A raw cast truncates
            // toward zero (a dragged 3.7 lands on 3) and, outside i16's
            // range, is a silent wrap — 40000 would become -25536.
            Ok(Variant::Vector3int16 {
                x: to_i16(n[0]),
                y: to_i16(n[1]),
                z: to_i16(n[2]),
            })
        }
        Variant::Ray { .. } => {
            let n = parse_numbers(text, 6)?;
            Ok(Variant::Ray {
                origin: Vector3Data {
                    x: n[0],
                    y: n[1],
                    z: n[2],
                },
                direction: Vector3Data {
                    x: n[3],
                    y: n[4],
                    z: n[5],
                },
            })
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
        Variant::CFrame(frame) => parse_cframe(frame, text).map(Variant::CFrame),
        Variant::OptionalCFrame(frame) => parse_optional_cframe(frame, text),
        Variant::NumberRange(_) => {
            let n = parse_numbers(text, 2)?;
            Ok(Variant::NumberRange(NumberRange {
                min: n[0],
                max: n[1],
            }))
        }
        Variant::Rect(_) => {
            let n = parse_numbers(text, 4)?;
            Ok(Variant::Rect(Rect {
                min: Vector2Data { x: n[0], y: n[1] },
                max: Vector2Data { x: n[2], y: n[3] },
            }))
        }
        Variant::Font(_) => parse_font(text).map(Variant::Font),
        Variant::NumberSequence(_) => parse_number_sequence(text).map(Variant::NumberSequence),
        Variant::ColorSequence(current) => {
            parse_color_sequence(current, text).map(Variant::ColorSequence)
        }
        Variant::PhysicalProperties(current) => parse_physical(current, text),
        other => Err(format!("{} is read-only", type_name(other))),
    }
}

/// The sequence `text` spells, for a caller holding the text alone — the
/// row's own preview and `shell::sequence_panel`, which both need the
/// keypoints rather than a string. `color` picks which of the two shapes to
/// read it as, the same way [`crate::properties::EditKind::Sequence`] carries
/// it. Colour envelopes come back zeroed, since text never carries them (see
/// [`sequence::parse_color_sequence`]) — a preview does not use them, and the
/// panel reads the real ones off the value it opened on.
pub(crate) fn sequence_value(color: bool, text: &str) -> Option<Variant> {
    if color {
        parse_color_sequence(
            &rbx_dom::ColorSequence {
                keypoints: Vec::new(),
            },
            text,
        )
        .map(Variant::ColorSequence)
        .ok()
    } else {
        parse_number_sequence(text)
            .map(Variant::NumberSequence)
            .ok()
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

/// A `CFrame`, in as much of it as the text gives: three numbers are a
/// position, nine are a rotation and twelve are both, in the order Roblox's
/// own `CFrame.new(x, y, z, R00, R01, R02, R10, R11, R12, R20, R21, R22)`
/// takes them — row by row (`creator-docs`,
/// `reference/engine/datatypes/CFrame.yaml`). Whichever half is left out is
/// carried through from `current` untouched, so typing a position into the
/// Properties panel never loses a part's facing and a viewport rotate never
/// moves it.
/// A `CFrame` as the panel shows it: three position numbers, then the three
/// orientation angles in degrees (see [`orientation`]).
fn cframe_text(frame: &CFrameData) -> String {
    let [x, y, z] = orientation::to_degrees(&frame.rotation);
    format!(
        "{}, {}, {}, {}, {}, {}",
        frame.position.x, frame.position.y, frame.position.z, x, y, z
    )
}

/// Four shapes, by how many numbers were given: 3 is a position, 6 is a
/// position plus orientation degrees (what the panel submits), 9 is a raw
/// rotation matrix, 12 is a position plus a raw matrix.
///
/// The 6 case carries the rule this whole module exists for. The panel's
/// fields always submit all six numbers, whichever one was typed in — so
/// the angles are compared against what the row was *showing*, and the
/// stored matrix is left **byte-identical** unless they actually changed.
/// Without that, nudging a part's X position would quietly rewrite its
/// rotation through degrees and back: a lossy trip (nine numbers do not fit
/// in three) that at gimbal lock can land on a different orientation
/// entirely.
fn parse_cframe(current: &CFrameData, text: &str) -> Result<CFrameData, String> {
    let (position, rotation) = match count_numbers(text) {
        3 => (Some(parse_numbers(text, 3)?), None),
        6 => {
            let n = parse_numbers(text, 6)?;
            let typed = [n[3], n[4], n[5]];
            let rotation =
                (!orientation::same_angles(typed, orientation::to_degrees(&current.rotation)))
                    .then(|| orientation::from_degrees(typed).to_vec());
            (Some(n[..3].to_vec()), rotation)
        }
        9 => (None, Some(parse_numbers(text, 9)?)),
        _ => {
            let n = parse_numbers(text, 12)?;
            (Some(n[..3].to_vec()), Some(n[3..].to_vec()))
        }
    };

    Ok(CFrameData {
        position: position.map_or(current.position, |n| Vector3Data {
            x: n[0],
            y: n[1],
            z: n[2],
        }),
        rotation: rotation.map_or(current.rotation, |n| std::array::from_fn(|term| n[term])),
    })
}

fn to_i16(value: f32) -> i16 {
    value
        .round()
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

/// `count` comma-separated booleans, in the order the flag labels name.
fn parse_flags(text: &str, count: usize) -> Result<Vec<bool>, String> {
    // Trimmed, because the text these parse is the same comma-joined form
    // every other composite uses — with a space after each comma.
    let flags: Result<Vec<bool>, String> = text
        .split(',')
        .map(|part| parse_bool(part.trim()))
        .collect();
    let flags = flags?;
    if flags.len() != count {
        return Err(format!("expected {count} flags, got {}", flags.len()));
    }
    Ok(flags)
}

/// How many comma-separated terms `text` holds, which is what picks between
/// [`parse_cframe`]'s three shapes. Counting rather than trying each in turn
/// keeps the error a mistyped value produces pointed at the length the user
/// clearly meant.
fn count_numbers(text: &str) -> usize {
    text.split(',').count()
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
        Variant::Rect(_) => "Rect",
        Variant::PhysicalProperties(_) => "PhysicalProperties",
        Variant::SharedString(_) => "SharedString",
        Variant::UniqueId(_) => "UniqueId",
        Variant::SecurityCapabilities(_) => "SecurityCapabilities",
        Variant::Content(_) => "Content",
        Variant::Unknown { .. } => "Unknown",
        _ => "this type",
    }
}

#[cfg(test)]
mod tests;
