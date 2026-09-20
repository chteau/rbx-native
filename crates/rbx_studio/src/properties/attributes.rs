//! Custom `Instance` attributes and `CollectionService` tags: the DOM the
//! Properties panel's Attributes/Tags section reads and writes.
//!
//! Nothing here renders; `shell::attributes_panel` does that, and this
//! module is pure DOM in and DOM out so it can be tested without a window —
//! the same split `style_editor`/`shell::style_panel` already use.
//!
//! Both attribute values and tag names round-trip through
//! `rbx_dom::attributes`' own `decode`/`encode`/`tags`/`encode_tags`; this
//! module only adds the parts that crate does not own: name/type validation
//! (`Instance:SetAttribute`'s documented rules), duplicate handling, and the
//! `WeakDom` reads and writes those need.

use std::collections::BTreeMap;

use rbx_dom::{
    CFrameData, Color3Data, Font, FontStyle, NumberRange, Rect, Ref, UDim, UDim2, Variant,
    Vector2Data, Vector3Data, WeakDom,
};
use rbx_reflection::ReflectionDatabase;

use super::{value_edit_kind, EditKind};

pub(crate) const ATTRIBUTES_PROPERTY: &str = "AttributesSerialize";
pub(crate) const TAGS_PROPERTY: &str = "Tags";

/// `Variant::Unknown`'s wire type id for a string-shaped blob — the one id
/// XML's `BinaryString` round-trips (see `rbx_xml::value::STRING_TYPE_ID`
/// and `style_editor`'s identical constant), so a blob written back under it
/// survives a save in either format. A `Variant::String` would work for the
/// binary writer too (see `rbx_binary::serialize::prop::scalar`), but the
/// raw bytes of an encoded attribute blob are essentially never valid UTF-8
/// (they're packed floats), so `Variant::String` — which can only hold valid
/// UTF-8 — is not a safe choice here the way it would be for an actual
/// string property.
const STRING_TYPE_ID: u8 = 0x01;

/// `Instance:SetAttribute`'s own ceiling (creator-docs, "Limitations"): a
/// name over this length is refused before it ever reaches an encode.
const MAX_ATTRIBUTE_NAME_LEN: usize = 100;

/// Every `PropertyRow::name` this module hands out is prefixed with this so
/// `shell::edit::apply_edit` can tell an attribute's value apart from a real
/// property of the same spelling and route it here instead of to
/// `WeakDom::set_property` — the same trick `properties::edit::FOLDER_COLOR_PROPERTY`
/// already uses for a row backed by something other than a DOM property.
const ROW_PREFIX: &str = "Attribute:";

/// The row name [`row_name`] would build for `attribute`, so a caller
/// holding a `PropertyRow`-shaped commit target can tell an attribute row
/// apart from an ordinary property row.
pub(crate) fn row_name(attribute: &str) -> String {
    format!("{ROW_PREFIX}{attribute}")
}

/// The attribute name a row built by [`row_name`] is for, or `None` when
/// `row` is an ordinary property row instead.
pub(crate) fn attribute_of_row(row: &str) -> Option<&str> {
    row.strip_prefix(ROW_PREFIX)
}

/// The Roblox attribute types this editor can create — every type
/// `Instance:SetAttribute` accepts (`studio/properties.md#instance-attributes`)
/// except two left out deliberately:
///
/// - `NumberSequence`/`ColorSequence` have no Properties-panel editor yet —
///   `ROADMAP.md`'s "eight `Variant` types" bullet is explicit that each is
///   its own PR, and this one is not it. An attribute already holding either
///   (from a file authored elsewhere) still decodes and renders, read-only,
///   through the ordinary fallback — it just cannot be *created* here.
pub(crate) const ATTRIBUTE_TYPES: &[&str] = &[
    "String",
    "Boolean",
    "Number",
    "Color3",
    "UDim",
    "UDim2",
    "Vector2",
    "Vector3",
    "CFrame",
    "NumberRange",
    "Rect",
    "BrickColor",
    "Font",
];

/// The value a freshly created attribute of `type_name` starts with —
/// `type_name` must be one of [`ATTRIBUTE_TYPES`]'s own spellings.
pub(crate) fn default_value(type_name: &str) -> Option<Variant> {
    Some(match type_name {
        "String" => Variant::String(String::new()),
        "Boolean" => Variant::Bool(false),
        // Roblox's own "number" attribute type has no int/float split the
        // way Lua itself doesn't; `Float64` is the widest of this crate's
        // three numeric variants, so it loses the least starting out.
        "Number" => Variant::Float64(0.0),
        "Color3" => Variant::Color3(Color3Data {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        }),
        "UDim" => Variant::UDim(UDim {
            scale: 0.0,
            offset: 0,
        }),
        "UDim2" => Variant::UDim2(UDim2 {
            x: UDim {
                scale: 0.0,
                offset: 0,
            },
            y: UDim {
                scale: 0.0,
                offset: 0,
            },
        }),
        "Vector2" => Variant::Vector2(Vector2Data { x: 0.0, y: 0.0 }),
        "Vector3" => Variant::Vector3(Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        "CFrame" => Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: rbx_dom::rotation::IDENTITY,
        }),
        "NumberRange" => Variant::NumberRange(NumberRange { min: 0.0, max: 0.0 }),
        "Rect" => Variant::Rect(Rect {
            min: Vector2Data { x: 0.0, y: 0.0 },
            max: Vector2Data { x: 0.0, y: 0.0 },
        }),
        "BrickColor" => Variant::BrickColor(1),
        "Font" => Variant::Font(Font {
            family: "rbxasset://fonts/families/SourceSansPro.json".to_owned(),
            weight: 400,
            style: FontStyle::Normal,
            cached_face_id: None,
        }),
        _ => return None,
    })
}

/// Every attribute on `reference`, decoded from its blob — empty for a
/// reference the DOM no longer holds.
pub(crate) fn attributes(dom: &WeakDom, reference: Ref) -> BTreeMap<String, Variant> {
    dom.get(reference)
        .map(|instance| rbx_dom::attributes::decode(instance.properties().get(ATTRIBUTES_PROPERTY)))
        .unwrap_or_default()
}

/// Every tag on `reference` — empty for a reference the DOM no longer holds.
pub(crate) fn tags(dom: &WeakDom, reference: Ref) -> Vec<String> {
    dom.get(reference)
        .map(|instance| {
            rbx_dom::attributes::tags(instance.properties().get(TAGS_PROPERTY))
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// [`attributes`] narrowed to the names the Properties panel's filter box
/// matches, through the very same [`crate::properties::matches`] every
/// ordinary property row is narrowed by — the box searches one list, not two
/// that can drift apart.
pub(crate) fn attributes_matching(
    dom: &WeakDom,
    reference: Ref,
    filter: &str,
) -> BTreeMap<String, Variant> {
    attributes(dom, reference)
        .into_iter()
        .filter(|(name, _)| crate::properties::matches(name, filter))
        .collect()
}

/// [`tags`], narrowed the same way [`attributes_matching`] narrows
/// attributes.
pub(crate) fn tags_matching(dom: &WeakDom, reference: Ref, filter: &str) -> Vec<String> {
    tags(dom, reference)
        .into_iter()
        .filter(|tag| crate::properties::matches(tag, filter))
        .collect()
}

/// The `EditKind` an attribute's current value should edit through — the
/// exact same mapping an ordinary property of that type gets (see
/// `properties::value_edit_kind`), so the value routes through the
/// Properties panel's existing per-type widgets rather than a parallel set.
/// `None` for a type with no editor (an attribute this editor did not
/// create — `NumberSequence`/`ColorSequence` — or one this crate cannot
/// parse text back into at all), which keeps the row read-only exactly like
/// `Properties::edit_kind` does for the same case.
pub(crate) fn edit_kind(value: &Variant) -> Option<EditKind> {
    let text = super::edit::edit_text(value)?;
    Some(value_edit_kind(value, text))
}

/// Whether `name` is a legal attribute name (`Instance:SetAttribute`,
/// creator-docs "Limitations"): alphanumeric plus `.`, `-`, `/`, `_`; no
/// spaces or other symbols; 100 characters or fewer; and never starting with
/// `RBX`, which Roblox reserves for its own core scripts. Whether that
/// prefix check is case-sensitive, and whether "alphanumeric" means ASCII or
/// any Unicode letter/digit, is not stated by the docs beyond their own
/// literal spelling — this allows any Unicode alphanumeric rather than
/// silently rejecting something a real client might accept.
pub(crate) fn validate_attribute_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("an attribute needs a name".to_owned());
    }
    if name.chars().count() > MAX_ATTRIBUTE_NAME_LEN {
        return Err(format!(
            "attribute names are {MAX_ATTRIBUTE_NAME_LEN} characters or fewer"
        ));
    }
    if name.starts_with("RBX") {
        return Err("attribute names cannot start with \"RBX\" (reserved by Roblox)".to_owned());
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '/' | '_'))
    {
        return Err(
            "attribute names allow only letters, numbers, '.', '-', '/' and '_'".to_owned(),
        );
    }
    Ok(())
}

fn gone() -> String {
    "the instance no longer exists".to_owned()
}

/// A new attribute named `name` holding `value`. Refused when the name fails
/// [`validate_attribute_name`] or already names an attribute on this
/// instance — `SetAttribute` itself would just overwrite, but this panel's
/// "add" is a distinct action from editing an existing row's value, and
/// silently repointing an existing attribute from an "add" click would be a
/// surprising way to lose a value.
pub(crate) fn add_attribute(
    dom: &mut WeakDom,
    reference: Ref,
    name: &str,
    value: Variant,
) -> Result<(), String> {
    validate_attribute_name(name)?;
    let instance = dom.get(reference).ok_or_else(gone)?;
    let mut current = rbx_dom::attributes::decode(instance.properties().get(ATTRIBUTES_PROPERTY));
    if current.contains_key(name) {
        return Err(format!("{name:?} is already an attribute on this instance"));
    }
    current.insert(name.to_owned(), value);
    write_attributes(dom, reference, &current)
}

/// Drops one attribute. Refused when `name` is not currently one of this
/// instance's attributes.
pub(crate) fn remove_attribute(
    dom: &mut WeakDom,
    reference: Ref,
    name: &str,
) -> Result<(), String> {
    let mut current = attributes(dom, reference);
    if current.remove(name).is_none() {
        return Err(format!("{name:?} is not an attribute on this instance"));
    }
    write_attributes(dom, reference, &current)
}

/// Renames one attribute in place, keeping its value. A no-op when
/// `new_name` is exactly `old_name`; refused when `new_name` fails
/// [`validate_attribute_name`], already names a different attribute here, or
/// `old_name` is not currently one of this instance's attributes.
pub(crate) fn rename_attribute(
    dom: &mut WeakDom,
    reference: Ref,
    old_name: &str,
    new_name: &str,
) -> Result<(), String> {
    let mut current = attributes(dom, reference);
    if !current.contains_key(old_name) {
        return Err(format!("{old_name:?} is not an attribute on this instance"));
    }
    if old_name == new_name {
        return Ok(());
    }
    validate_attribute_name(new_name)?;
    if current.contains_key(new_name) {
        return Err(format!(
            "{new_name:?} is already an attribute on this instance"
        ));
    }
    let value = current.remove(old_name).expect("checked above");
    current.insert(new_name.to_owned(), value);
    write_attributes(dom, reference, &current)
}

/// Parses `text` into a value shaped like attribute `name`'s current one —
/// the same textual commit path an ordinary property row takes
/// (`properties::edit::parse`) — and writes it back. Attributes are never
/// `Variant::Enum` (not one of the types `Instance:SetAttribute` accepts),
/// so `parse`'s reflection lookup is never exercised here; the empty
/// class/property-name arguments are inert placeholders for the one arm that
/// would need them.
pub(crate) fn set_attribute_value(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    name: &str,
    text: &str,
) -> Result<(), String> {
    let mut current = attributes(dom, reference);
    let existing = current
        .get(name)
        .ok_or_else(|| format!("{name:?} is not an attribute on this instance"))?
        .clone();
    let value = super::edit::parse(&existing, db, "", "", text)?;
    current.insert(name.to_owned(), value);
    write_attributes(dom, reference, &current)
}

fn write_attributes(
    dom: &mut WeakDom,
    reference: Ref,
    attributes: &BTreeMap<String, Variant>,
) -> Result<(), String> {
    // Attributes are never `Enum` (see `set_attribute_value`), so `encode`'s
    // callback is never actually invoked; it exists only to satisfy the
    // signature `style_editor::write_properties` also has to fill in.
    let raw = rbx_dom::attributes::encode(attributes, |_| None).ok_or_else(|| {
        "one of these attributes holds a type this editor cannot write back".to_owned()
    })?;
    dom.set_property(
        reference,
        ATTRIBUTES_PROPERTY,
        Variant::Unknown {
            type_id: STRING_TYPE_ID,
            raw,
        },
    )
    .map(|_| ())
    .map_err(|err| err.to_string())
}

/// Applies a tag, matching `CollectionService:AddTag`'s own documented
/// behaviour: doing nothing if the tag is already applied. An empty tag is
/// refused — this crate's own wire format (`rbx_dom::attributes::tags`)
/// cannot tell an empty tag apart from no tag at all, so one would silently
/// vanish the next time the blob round-trips through a save/reload, which is
/// worse than refusing it up front. Whether the real client itself refuses
/// `AddTag(instance, "")` outright is not documented and was not verified
/// here.
pub(crate) fn add_tag(dom: &mut WeakDom, reference: Ref, tag: &str) -> Result<(), String> {
    if tag.is_empty() {
        return Err("a tag needs a name".to_owned());
    }
    if tag.contains('\0') {
        return Err("a tag cannot contain a NUL character".to_owned());
    }
    let instance = dom.get(reference).ok_or_else(gone)?;
    let mut current: Vec<String> =
        rbx_dom::attributes::tags(instance.properties().get(TAGS_PROPERTY))
            .into_iter()
            .map(str::to_owned)
            .collect();
    if current.iter().any(|existing| existing == tag) {
        return Ok(());
    }
    current.push(tag.to_owned());
    write_tags(dom, reference, &current)
}

/// Removes one tag. Refused when `tag` is not currently applied.
pub(crate) fn remove_tag(dom: &mut WeakDom, reference: Ref, tag: &str) -> Result<(), String> {
    let mut current = tags(dom, reference);
    let before = current.len();
    current.retain(|existing| existing != tag);
    if current.len() == before {
        return Err(format!("{tag:?} is not one of this instance's tags"));
    }
    write_tags(dom, reference, &current)
}

fn write_tags(dom: &mut WeakDom, reference: Ref, tags: &[String]) -> Result<(), String> {
    let borrowed: Vec<&str> = tags.iter().map(String::as_str).collect();
    let text = rbx_dom::attributes::encode_tags(&borrowed);
    dom.set_property(reference, TAGS_PROPERTY, Variant::String(text))
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests;
