//! Documents which `Variant` kinds the reader actually produces for the two real
//! fixtures, per the serializer's coverage requirement: every kind here must have an
//! encoder in `serialize::prop`, and anything outside this list is fair game to leave
//! `SerializeError::Unsupported`.

mod support;

use std::collections::BTreeSet;

use rbx_binary::deserialize;
use rbx_dom::Variant;

use support::{all_refs, FPS, TEST_PLACE};

fn kind_name(v: &Variant) -> &'static str {
    match v {
        Variant::String(_) => "String",
        Variant::Bool(_) => "Bool",
        Variant::Int32(_) => "Int32",
        Variant::Int64(_) => "Int64",
        Variant::Float32(_) => "Float32",
        Variant::Float64(_) => "Float64",
        Variant::BrickColor(_) => "BrickColor",
        Variant::Color3(_) => "Color3",
        Variant::Color3uint8 { .. } => "Color3uint8",
        Variant::Vector2(_) => "Vector2",
        Variant::Vector3(_) => "Vector3",
        Variant::Vector3int16 { .. } => "Vector3int16",
        Variant::Ray { .. } => "Ray",
        Variant::Faces(_) => "Faces",
        Variant::Axes(_) => "Axes",
        Variant::CFrame(_) => "CFrame",
        Variant::OptionalCFrame(_) => "OptionalCFrame",
        Variant::Enum(_) => "Enum",
        Variant::Ref(_) => "Ref",
        Variant::NumberSequence(_) => "NumberSequence",
        Variant::ColorSequence(_) => "ColorSequence",
        Variant::NumberRange(_) => "NumberRange",
        Variant::Rect(_) => "Rect",
        Variant::PhysicalProperties(_) => "PhysicalProperties",
        Variant::SharedString(_) => "SharedString",
        Variant::UDim(_) => "UDim",
        Variant::UDim2(_) => "UDim2",
        Variant::UniqueId(_) => "UniqueId",
        Variant::Font(_) => "Font",
        Variant::SecurityCapabilities(_) => "SecurityCapabilities",
        Variant::Content(_) => "Content",
        Variant::Unknown { .. } => "Unknown",
    }
}

// Kinds observed in FPS.rbxm and TestPlace.rbxl as of this writing (see the crate's
// serialize module doc comment for the up-to-date coverage list this backs).
const EXPECTED_IN_FIXTURES: &[&str] = &[
    "Bool",
    "BrickColor",
    "CFrame",
    "Color3",
    "Color3uint8",
    "ColorSequence",
    "Content",
    "Enum",
    "Float32",
    "Float64",
    "Font",
    "Int32",
    "Int64",
    "NumberRange",
    "NumberSequence",
    "OptionalCFrame",
    "PhysicalProperties",
    "Rect",
    "Ref",
    "SecurityCapabilities",
    "String",
    "UDim",
    "UDim2",
    "UniqueId",
    "Unknown",
    "Vector2",
    "Vector3",
];

#[test]
fn fixtures_only_use_the_documented_variant_kinds() {
    let mut seen = BTreeSet::new();
    for bytes in [FPS, TEST_PLACE] {
        let dom = deserialize(bytes).unwrap();
        for referent in all_refs(&dom) {
            let Some(instance) = dom.get(referent) else {
                continue;
            };
            for value in instance.properties().values() {
                seen.insert(kind_name(value));
            }
        }
    }

    let expected: BTreeSet<&str> = EXPECTED_IN_FIXTURES.iter().copied().collect();
    assert_eq!(
        seen, expected,
        "update EXPECTED_IN_FIXTURES (and the serializer's coverage) if this fails"
    );
}
