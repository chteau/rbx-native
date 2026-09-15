//! Dispatches a `Properties` child element to its decoded `Variant`, by tag name.
//!
//! xml.md itself recommends resolving by reflection rather than by tag name, since
//! encoders MAY rename type elements; this crate has no reflection database wired
//! in, so it dispatches on Roblox's own conventional names instead, which its own
//! encoders commit to for compatibility (see xml.md's "Type Elements" section).

mod cframe;
mod content;
mod flags;
mod font;
mod identity;
mod refs;
mod scalar;
mod sequence;
mod udim;
mod vector;

use std::collections::HashMap;

use rbx_dom::{Ref, Variant};

use crate::xml_tree::Node;

// Reused as the `Unknown` fallback's `type_id` for a value that has no equivalent
// binary-format type id: every real one documented in `rbx_binary` fits in one
// byte well below this, so it can never collide with a genuine type.
pub(crate) const UNKNOWN_TAG_TYPE_ID: u8 = 0xFF;
// Matches the binary format's own String type id (0x01), reused here so a
// non-UTF-8 BinaryString/SharedString payload degrades exactly the way the
// binary parser's own string decoder degrades a non-UTF-8 String property.
pub(crate) const STRING_TYPE_ID: u8 = 0x01;

/// Referent and shared-string lookup tables threaded through every decoder that
/// needs to resolve a `Ref`, a `Content`'s `Ref` child, or a `SharedString`/`NetAssetRef`.
pub(crate) struct Ctx<'a> {
    pub(crate) referents: &'a HashMap<String, Ref>,
    pub(crate) shared: &'a HashMap<String, Vec<u8>>,
}

/// Decodes one `Properties` child element into a `Variant`.
///
/// Returns `None` when the property has no value to store (a null `Ref`, or a
/// `Ref` pointing at a referent this document never defines), mirroring how the
/// binary decoder represents "no value" for the same cases.
pub(crate) fn decode(node: &Node, ctx: &Ctx<'_>) -> Option<Variant> {
    // `Ref` is the one type whose absence is meaningful (a null or dangling
    // referent), so it is resolved before the rest, which always produce a value.
    if node.tag == "Ref" {
        return refs::reference(node.text_trim(), ctx);
    }

    Some(match node.tag.as_str() {
        "string" | "ProtectedString" => scalar::string_value(node.text.as_bytes()),
        "BinaryString" => scalar::binary_string(node),
        "bool" => scalar::bool_value(node.text_trim()),
        "int" => scalar::int32(node.text_trim()),
        "BrickColor" => scalar::brick_color(node.text_trim()),
        "int64" => scalar::int64(node.text_trim()),
        "float" => scalar::float32(node.text_trim()),
        "double" => scalar::float64(node.text_trim()),
        "token" => scalar::enum_value(node.text_trim()),
        "Vector2" => vector::vector2_value(node),
        "Vector3" => vector::vector3_value(node),
        "Vector3int16" => vector::vector3int16(node),
        "Color3" => vector::color3(node),
        "Color3uint8" => vector::color3_uint8(node.text_trim()),
        "Rect2D" => vector::rect2d(node),
        "Ray" => vector::ray(node),
        "CoordinateFrame" => cframe::cframe_value(node),
        "OptionalCoordinateFrame" => cframe::optional_cframe(node),
        "UDim" => udim::udim(node),
        "UDim2" => udim::udim2(node),
        "NumberRange" => sequence::number_range(node.text_trim()),
        "NumberSequence" => sequence::number_sequence(node.text_trim()),
        "ColorSequence" => sequence::color_sequence(node.text_trim()),
        "PhysicalProperties" => flags::physical_properties(node),
        "Axes" => flags::axes(node),
        "Faces" => flags::faces(node),
        "SecurityCapabilities" => flags::security_capabilities(node.text_trim()),
        "UniqueId" => identity::unique_id(node.text_trim()),
        "Font" => font::font(node),
        "Content" => content::content(node, ctx),
        "ContentId" => content::content_id(node),
        "SharedString" | "NetAssetRef" => refs::shared_string(node.text_trim(), ctx),
        // Unrecognized type element: kept rather than dropped, storing the tag's
        // inner text so nothing about the property is silently lost.
        _ => Variant::Unknown {
            type_id: UNKNOWN_TAG_TYPE_ID,
            raw: node.text.as_bytes().to_vec(),
        },
    })
}
