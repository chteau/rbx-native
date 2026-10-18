use super::*;
use rbx_dom::{FontStyle, UniqueId};

fn target(r: Ref) -> Option<Target> {
    Some(Target {
        id: format!("id{}", r.value()),
        p: format!("Workspace.P{}", r.value()),
    })
}

fn resolve(t: &Target) -> Option<Ref> {
    t.id.strip_prefix("id")?.parse().ok().map(Ref::new)
}

fn round(v: Variant) {
    let json = encode(&v, &target).expect("sendable");
    let text = serde_json::to_string(&json).unwrap();
    let back: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(decode(&back, &resolve), Some(v));
}

#[test]
fn every_sendable_kind_round_trips() {
    let c3 = Color3Data {
        r: 0.1,
        g: 0.333_333_34,
        b: 1.0,
    };
    let v3 = Vector3Data {
        x: 0.1,
        y: -2.5e-7,
        z: 1e10,
    };
    let cf = CFrameData {
        position: v3,
        rotation: [1.0, 0.0, 0.0, 0.0, 0.6, -0.8, 0.0, 0.8, 0.6],
    };
    let all = vec![
        Variant::String("hé\"llo".into()),
        Variant::Bool(true),
        Variant::Int32(-7),
        Variant::Int64(i64::MAX),
        Variant::Float32(0.1),
        Variant::Float64(0.1),
        Variant::BrickColor(194),
        Variant::Color3(c3),
        Variant::Color3uint8 { r: 1, g: 2, b: 255 },
        Variant::Vector2(Vector2Data { x: 0.1, y: 0.2 }),
        Variant::Vector3(v3),
        Variant::Vector3int16 {
            x: -1,
            y: 2,
            z: 32767,
        },
        Variant::Ray {
            origin: v3,
            direction: Vector3Data {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
        },
        Variant::Faces(Faces {
            front: true,
            bottom: false,
            left: true,
            back: false,
            top: true,
            right: false,
        }),
        Variant::Axes(Axes {
            x: true,
            y: false,
            z: true,
        }),
        Variant::CFrame(cf),
        Variant::OptionalCFrame(Some(cf)),
        Variant::OptionalCFrame(None),
        Variant::Enum(3),
        Variant::Ref(Ref::new(42)),
        Variant::NumberSequence(NumberSequence {
            keypoints: vec![
                NumberSequenceKeypoint {
                    time: 0.0,
                    value: 0.1,
                    envelope: 0.0,
                },
                NumberSequenceKeypoint {
                    time: 1.0,
                    value: 0.7,
                    envelope: 0.2,
                },
            ],
        }),
        Variant::ColorSequence(ColorSequence {
            keypoints: vec![ColorSequenceKeypoint {
                time: 0.5,
                color: c3,
                envelope: 0.0,
            }],
        }),
        Variant::NumberRange(NumberRange { min: 0.1, max: 9.9 }),
        Variant::Rect(Rect {
            min: Vector2Data { x: 0.1, y: 0.2 },
            max: Vector2Data { x: 3.0, y: 4.0 },
        }),
        Variant::PhysicalProperties(PhysicalProperties::Default),
        Variant::PhysicalProperties(PhysicalProperties::Custom {
            density: 0.7,
            friction: 0.3,
            elasticity: 0.5,
            friction_weight: 1.0,
            elasticity_weight: 2.0,
        }),
        Variant::UDim(UDim {
            scale: 0.25,
            offset: -12,
        }),
        Variant::UDim2(UDim2 {
            x: UDim {
                scale: 0.1,
                offset: 5,
            },
            y: UDim {
                scale: 1.0,
                offset: i32::MAX,
            },
        }),
        Variant::Font(Font {
            family: "rbxasset://fonts/families/Arimo.json".into(),
            weight: 700,
            style: FontStyle::Italic,
            cached_face_id: None,
        }),
        Variant::SecurityCapabilities(u64::from(u32::MAX)),
        Variant::Content(Content::Uri("rbxassetid://1".into())),
    ];
    for v in all {
        round(v);
    }
}

#[test]
fn unsendable_kinds_give_none() {
    for v in [
        Variant::SharedString(1),
        Variant::UniqueId(UniqueId {
            index: 1,
            time: 2,
            random: 3,
        }),
        Variant::Unknown {
            type_id: 9,
            raw: vec![1],
        },
        Variant::Content(Content::None),
        Variant::Content(Content::Object(Ref::new(1))),
        Variant::Float32(f32::NAN),
    ] {
        assert_eq!(encode(&v, &target), None, "{v:?}");
    }
}

#[test]
fn malformed_json_gives_none() {
    for j in [
        json!(null),
        json!({"t": "Vector3", "v": [1, 2]}),
        json!({"t": "Vector3", "v": "x"}),
        json!({"t": "Bool", "v": 1}),
        json!({"t": "Nope", "v": 1}),
        json!({"t": "Int32", "v": 1e12}),
        json!({"t": "PhysicalProperties", "v": "Other"}),
        json!({"t": "Ref", "v": null}),
        json!({"t": "Ref", "v": {"id": "zzz", "p": ""}}),
        json!({"t": "Font", "v": {"family": "a"}}),
        json!({"v": 1}),
    ] {
        assert_eq!(decode(&j, &resolve), None, "{j}");
    }
}
