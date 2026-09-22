use rbx_dom::{CFrameData, Color3Data, Content, UDim, UDim2, Variant, Vector2Data, Vector3Data};
use rmpv::Value;

use super::*;

fn tagged(tag: &str, payload: Value) -> Value {
    Value::Map(vec![(Value::from(tag), payload)])
}

fn arr(values: Vec<Value>) -> Value {
    Value::Array(values)
}

#[test]
fn string_round_trips() {
    let variant = Variant::String("hello".to_owned());
    let encoded = encode(&variant).unwrap();
    assert_eq!(decode(&encoded), Some(variant));
}

#[test]
fn bool_decodes_from_the_msgpack_boolean_type() {
    let encoded = tagged("Bool", Value::Boolean(true));
    assert_eq!(decode(&encoded), Some(Variant::Bool(true)));
}

// The quirk `argon-roblox`'s own hand-rolled MsgPack encoder has: a whole
// number is written as a MsgPack *integer*, never a float, regardless of
// what type tag it's filed under — this is `Part.Size.X == 4.0` arriving as
// plain `4`, not `4.0`.
#[test]
fn a_float64_tagged_whole_number_decodes_even_when_the_wire_int_never_carries_a_dot() {
    let encoded = tagged("Float64", Value::from(4_i64));
    assert_eq!(decode(&encoded), Some(Variant::Float64(4.0)));
}

#[test]
fn a_float32_tagged_fractional_number_decodes_from_the_msgpack_float_family() {
    let encoded = tagged("Float32", Value::F64(0.5));
    assert_eq!(decode(&encoded), Some(Variant::Float32(0.5)));
}

#[test]
fn vector3_reads_the_three_element_array() {
    let encoded = tagged(
        "Vector3",
        arr(vec![Value::from(1), Value::from(2), Value::from(3)]),
    );
    assert_eq!(
        decode(&encoded),
        Some(Variant::Vector3(Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0
        }))
    );
}

#[test]
fn vector3_round_trips() {
    let variant = Variant::Vector3(Vector3Data {
        x: 1.5,
        y: -2.0,
        z: 0.0,
    });
    assert_eq!(decode(&encode(&variant).unwrap()), Some(variant));
}

#[test]
fn color3uint8_is_distinct_from_color3() {
    let encoded = tagged(
        "Color3uint8",
        arr(vec![Value::from(163), Value::from(162), Value::from(165)]),
    );
    assert_eq!(
        decode(&encoded),
        Some(Variant::Color3uint8 {
            r: 163,
            g: 162,
            b: 165
        })
    );
}

#[test]
fn color3_is_the_float_variant() {
    let variant = Variant::Color3(Color3Data {
        r: 1.0,
        g: 0.5,
        b: 0.0,
    });
    assert_eq!(decode(&encode(&variant).unwrap()), Some(variant));
}

#[test]
fn cframe_reads_position_and_the_row_major_orientation_matrix() {
    let encoded = tagged(
        "CFrame",
        Value::Map(vec![
            (
                Value::from("position"),
                arr(vec![Value::from(0), Value::from(0), Value::from(0)]),
            ),
            (
                Value::from("orientation"),
                arr(vec![
                    arr(vec![Value::from(1), Value::from(0), Value::from(0)]),
                    arr(vec![Value::from(0), Value::from(1), Value::from(0)]),
                    arr(vec![Value::from(0), Value::from(0), Value::from(1)]),
                ]),
            ),
        ]),
    );
    assert_eq!(
        decode(&encoded),
        Some(Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }))
    );
}

#[test]
fn cframe_round_trips() {
    let variant = Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    });
    assert_eq!(decode(&encode(&variant).unwrap()), Some(variant));
}

// Canonical properties collapse an enum-typed reflection `DataType` to the
// bare tag `Enum`, carrying only the ordinal — no enum-type name at all on
// the wire (that's `EnumItem`'s job, used only for generic Attributes,
// which this client doesn't decode through this path).
#[test]
fn enum_is_a_bare_ordinal_with_no_type_name_on_the_wire() {
    let encoded = tagged("Enum", Value::from(256));
    assert_eq!(decode(&encoded), Some(Variant::Enum(256)));
}

#[test]
fn udim2_reads_both_axes() {
    let encoded = tagged(
        "UDim2",
        arr(vec![
            arr(vec![Value::F64(0.5), Value::from(10)]),
            arr(vec![Value::F64(0.0), Value::from(-5)]),
        ]),
    );
    assert_eq!(
        decode(&encoded),
        Some(Variant::UDim2(UDim2 {
            x: UDim {
                scale: 0.5,
                offset: 10
            },
            y: UDim {
                scale: 0.0,
                offset: -5
            },
        }))
    );
}

#[test]
fn content_none_decodes_from_the_bare_string() {
    let encoded = tagged("Content", Value::from("None"));
    assert_eq!(decode(&encoded), Some(Variant::Content(Content::None)));
}

#[test]
fn content_uri_decodes_from_the_wrapped_form() {
    let encoded = tagged(
        "Content",
        Value::Map(vec![(Value::from("Uri"), Value::from("rbxassetid://1"))]),
    );
    assert_eq!(
        decode(&encoded),
        Some(Variant::Content(Content::Uri("rbxassetid://1".to_owned())))
    );
}

#[test]
fn content_uri_round_trips() {
    let variant = Variant::Content(Content::Uri("rbxassetid://42".to_owned()));
    assert_eq!(decode(&encode(&variant).unwrap()), Some(variant));
}

#[test]
fn content_object_has_nothing_to_encode_back_to() {
    // Matches `argon-roblox`'s own encoder, which errors on this shape too.
    assert_eq!(
        encode(&Variant::Content(Content::Object(rbx_dom::Ref::new(1)))),
        None
    );
}

#[test]
fn an_unrecognized_tag_decodes_to_nothing_rather_than_panicking() {
    let encoded = tagged("SomeFutureType", Value::Nil);
    assert_eq!(decode(&encoded), None);
}

#[test]
fn vector2_round_trips() {
    let variant = Variant::Vector2(Vector2Data { x: 1.0, y: -1.0 });
    assert_eq!(decode(&encode(&variant).unwrap()), Some(variant));
}
