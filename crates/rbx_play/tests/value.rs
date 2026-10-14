//! `rbx_play::value` against the shared reflection database. No network: the
//! reflection data is compiled in and every instance reference is a `Ref`
//! from a local `WeakDom`.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_play::value::{decode, decode_attribute, encode, encode_attribute};
use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Value};

const TARGET_ID: u64 = 7;

fn target_ref() -> Ref {
    let mut dom = WeakDom::new();
    dom.new_instance("Part", "Target", None)
}

fn instance(target: Ref) -> impl Fn(u64) -> Option<Ref> {
    move |id| (id == TARGET_ID).then_some(target)
}

fn id_of(target: Ref) -> impl Fn(Ref) -> Option<u64> {
    move |r| (r == target).then_some(TARGET_ID)
}

/// Integers and floats compare equal regardless of how serde_json stored them.
fn norm(value: &Value) -> Value {
    match value {
        Value::Number(n) => Value::from(n.as_f64().expect("finite number")),
        Value::Array(items) => Value::Array(items.iter().map(norm).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), norm(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Decodes `json`, encodes it again and decodes that: the variant must survive.
/// Returns the first decoded variant and its re-encoded JSON.
fn round_trip(json: &Value, value_type: &str) -> (Variant, Value) {
    let db = ReflectionDatabase::shared();
    let target = target_ref();
    let variant = decode(json, value_type, db, &instance(target)).expect("decodes");
    let encoded = encode(&variant, value_type, db, &id_of(target)).expect("encodes");
    let again = decode(&encoded, value_type, db, &instance(target)).expect("decodes again");
    assert_eq!(again, variant);
    (variant, encoded)
}

/// Round trip that also requires the re-encoded JSON to match the input.
fn check(json: Value, value_type: &str) -> Variant {
    let (variant, encoded) = round_trip(&json, value_type);
    assert_eq!(norm(&encoded), norm(&json));
    variant
}

#[test]
fn vector3_on_part_size() {
    let variant = check(json!({"t": "V3", "v": [1, 2, 3]}), "Vector3");
    assert!(matches!(
        variant,
        Variant::Vector3(v) if v.x == 1.0 && v.y == 2.0 && v.z == 3.0
    ));
}

#[test]
fn cframe_on_part_cframe_keeps_position_and_rotation() {
    let json = json!({"t": "CF", "v": [1, 2, 3, 0, 0, 1, 0, 1, 0, -1, 0, 0]});
    let variant = check(json, "CFrame");
    assert!(matches!(variant, Variant::CFrame(_)));
}

#[test]
fn color3_on_part_color() {
    let variant = check(json!({"t": "C3", "v": [0.5, 0.25, 1.0]}), "Color3");
    assert!(matches!(variant, Variant::Color3(_)));
}

#[test]
fn brickcolor_on_part_brickcolor() {
    let variant = check(json!({"t": "BC", "v": 194}), "BrickColor");
    assert!(matches!(variant, Variant::BrickColor(_)));
}

#[test]
fn udim2_on_frame_size() {
    let variant = check(json!({"t": "U2", "v": [0.5, 10, 0.25, -4]}), "UDim2");
    assert!(matches!(variant, Variant::UDim2(_)));
}

#[test]
fn rect_round_trips() {
    let variant = check(json!({"t": "R", "v": [0, 0, 100, 50]}), "Rect");
    assert!(matches!(variant, Variant::Rect(_)));
}

#[test]
fn number_range_on_particle_lifetime() {
    let variant = check(json!({"t": "NR", "v": [1, 2]}), "NumberRange");
    assert!(matches!(variant, Variant::NumberRange(_)));
}

#[test]
fn number_sequence_on_particle_size() {
    let json = json!({"t": "NS", "v": [[0, 1, 0], [1, 2, 0.5]]});
    let variant = check(json, "NumberSequence");
    assert!(matches!(variant, Variant::NumberSequence(_)));
}

#[test]
fn color_sequence_on_particle_color() {
    let json = json!({"t": "CS", "v": [[0, 1, 0, 0], [1, 0, 0, 1]]});
    let variant = check(json, "ColorSequence");
    assert!(matches!(variant, Variant::ColorSequence(_)));
}

#[test]
fn enum_on_part_material() {
    let variant = check(json!({"t": "E", "v": 256, "n": "Plastic"}), "Material");
    assert!(matches!(variant, Variant::Enum(_)));
}

#[test]
fn ref_to_probe_id_seven_resolves_and_encodes_back() {
    let db = ReflectionDatabase::shared();
    let target = target_ref();
    let json = json!({"t": "I", "v": 7});
    let variant = decode(&json, "BasePart", db, &instance(target)).expect("decodes");
    assert!(matches!(variant, Variant::Ref(r) if r == target));
    assert_eq!(encode(&variant, "BasePart", db, &id_of(target)), Some(json));
}

#[test]
fn ref_to_id_zero_is_none() {
    let db = ReflectionDatabase::shared();
    let target = target_ref();
    let json = json!({"t": "I", "v": 0});
    assert!(decode(&json, "BasePart", db, &instance(target)).is_none());
    // A second instance, so its ref differs from `target`'s (refs are
    // per-DOM counters: each fresh DOM's first instance has the same one).
    let mut dom = WeakDom::new();
    dom.new_instance("Folder", "First", None);
    let stray = dom.new_instance("Part", "Stray", None);
    assert_ne!(stray, target);
    let nil = encode(&Variant::Ref(stray), "BasePart", db, &id_of(target));
    assert_eq!(nil, Some(json!({"t": "I", "v": 0})));
}

#[test]
fn font_on_text_label_font_face() {
    let json = json!({
        "t": "F",
        "v": ["rbxasset://fonts/families/SourceSansPro.json", 400, "Normal"]
    });
    let variant = check(json, "Font");
    assert!(matches!(variant, Variant::Font(_)));
}

#[test]
fn faces_on_handles_faces() {
    let variant = check(json!({"t": "Fa", "v": ["Top", "Front"]}), "Faces");
    assert!(matches!(variant, Variant::Faces(_)));
}

#[test]
fn physical_properties_default_round_trips() {
    let db = ReflectionDatabase::shared();
    let target = target_ref();
    let json = json!({"t": "PP"});
    let variant = decode(&json, "PhysicalProperties", db, &instance(target)).expect("decodes");
    let encoded = encode(&variant, "PhysicalProperties", db, &id_of(target)).expect("encodes");
    let again = decode(&encoded, "PhysicalProperties", db, &instance(target)).expect("decodes");
    assert_eq!(again, variant);
}

#[test]
fn physical_properties_custom_round_trips() {
    let json = json!({"t": "PP", "v": [0.5, 0.25, 0.5, 1, 2]});
    let variant = check(json, "PhysicalProperties");
    assert!(matches!(variant, Variant::PhysicalProperties(_)));
}

#[test]
fn numbers_follow_declared_type() {
    let db = ReflectionDatabase::shared();
    let target = target_ref();
    let decode_with =
        |json: Value, value_type: &str| decode(&json, value_type, db, &instance(target));

    assert!(matches!(
        decode_with(json!(0.5), "float"),
        Some(Variant::Float32(f)) if f == 0.5
    ));
    assert!(matches!(
        decode_with(json!(true), "bool"),
        Some(Variant::Bool(true))
    ));
    assert!(matches!(
        decode_with(json!(12), "int"),
        Some(Variant::Int32(12))
    ));
}

#[test]
fn nan_tag_decodes_to_nan_float32() {
    let db = ReflectionDatabase::shared();
    let target = target_ref();
    let json = json!({"t": "n", "v": "nan"});
    let variant = decode(&json, "float", db, &instance(target)).expect("decodes");
    assert!(matches!(variant, Variant::Float32(f) if f.is_nan()));
}

#[test]
fn mismatched_json_is_none() {
    let db = ReflectionDatabase::shared();
    let target = target_ref();
    let json = json!("x");
    assert!(decode(&json, "Vector3", db, &instance(target)).is_none());
}

#[test]
fn attribute_numbers_strings_bools_and_vectors_decode() {
    assert!(matches!(
        decode_attribute(&json!(1.5)),
        Some(Variant::Float64(f)) if f == 1.5
    ));
    assert!(matches!(
        decode_attribute(&json!("x")),
        Some(Variant::String(s)) if s == "x"
    ));
    assert!(matches!(
        decode_attribute(&json!(true)),
        Some(Variant::Bool(true))
    ));
    assert!(matches!(
        decode_attribute(&json!({"t": "V3", "v": [1, 2, 3]})),
        Some(Variant::Vector3(_))
    ));
    assert!(decode_attribute(&Value::Null).is_none());
}

#[test]
fn attribute_values_encode_back() {
    assert_eq!(encode_attribute(&Variant::Float64(1.5)), Some(json!(1.5)));
    assert_eq!(
        encode_attribute(&Variant::String("x".to_owned())),
        Some(json!("x"))
    );
    assert_eq!(encode_attribute(&Variant::Bool(true)), Some(json!(true)));

    let vector = json!({"t": "V3", "v": [1, 2, 3]});
    let decoded = decode_attribute(&vector).expect("decodes");
    let encoded = encode_attribute(&decoded).expect("encodes");
    assert_eq!(norm(&encoded), norm(&vector));
}
