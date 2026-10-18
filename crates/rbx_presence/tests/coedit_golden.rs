//! Golden vectors for the Studio plugin's Luau port of the co-editing model.
//! `RBX_PRESENCE_REGEN=1` rewrites the fixture from the Rust values; otherwise
//! the fixture must equal what the Rust code produces now.

use std::path::PathBuf;

use rbx_dom::{
    Axes, CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Content, Faces,
    NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, Ref, UDim,
    UDim2, UniqueId, Variant, Vector2Data, Vector3Data,
};
use rbx_presence::coedit::chunk;
use rbx_presence::coedit::ids::{path_id, unique_hex};
use rbx_presence::coedit::merge::{merge3, text_hash};
use rbx_presence::coedit::value::{decode, encode};
use rbx_presence::coedit::{Clock, Op, Target};
use serde_json::{json, Map, Value};

const REF_ID: &str = "0123456789abcdef0123456789abcdef";
const REF_PATH: &str = "Workspace.Part";

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/team-presence/fixtures/coedit_vectors.json")
}

fn ref_target() -> Target {
    Target {
        id: REF_ID.to_string(),
        p: REF_PATH.to_string(),
    }
}

fn v3(x: f32, y: f32, z: f32) -> Vector3Data {
    Vector3Data { x, y, z }
}

fn cframe() -> CFrameData {
    CFrameData {
        position: v3(1.0, 2.5, -3.0),
        rotation: [1.0, 0.0, 0.0, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5],
    }
}

fn values() -> Value {
    let font = decode(
        &json!({"t": "Font", "v": {
            "family": "rbxasset://fonts/families/GothamSSm.json",
            "weight": 400,
            "style": 0
        }}),
        &|_| None,
    )
    .expect("font decodes");
    let list: Vec<(&str, Variant)> = vec![
        ("String", Variant::String("héllo \"world\"\n".to_string())),
        ("Bool", Variant::Bool(true)),
        ("Int32", Variant::Int32(-123456)),
        ("Int64", Variant::Int64(9_007_199_254_740_991)),
        ("Float32", Variant::Float32(1.5)),
        ("Float64", Variant::Float64(-0.125)),
        ("BrickColor", Variant::BrickColor(194)),
        (
            "Color3",
            Variant::Color3(Color3Data {
                r: 1.0,
                g: 0.5,
                b: 0.25,
            }),
        ),
        (
            "Color3uint8",
            Variant::Color3uint8 {
                r: 255,
                g: 128,
                b: 0,
            },
        ),
        ("Vector2", Variant::Vector2(Vector2Data { x: 1.5, y: -2.0 })),
        ("Vector3", Variant::Vector3(v3(1.0, 2.0, 3.5))),
        (
            "Vector3int16",
            Variant::Vector3int16 {
                x: -1,
                y: 2,
                z: 300,
            },
        ),
        (
            "Ray",
            Variant::Ray {
                origin: v3(1.0, 2.0, 3.0),
                direction: v3(0.0, -1.0, 0.0),
            },
        ),
        (
            "Faces",
            Variant::Faces(Faces {
                front: true,
                bottom: false,
                left: true,
                back: false,
                top: true,
                right: false,
            }),
        ),
        (
            "Axes",
            Variant::Axes(Axes {
                x: true,
                y: false,
                z: true,
            }),
        ),
        ("CFrame", Variant::CFrame(cframe())),
        ("OptionalCFrame", Variant::OptionalCFrame(Some(cframe()))),
        ("OptionalCFrameNil", Variant::OptionalCFrame(None)),
        ("Enum", Variant::Enum(3)),
        ("Ref", Variant::Ref(Ref::new(5))),
        (
            "NumberSequence",
            Variant::NumberSequence(NumberSequence {
                keypoints: vec![
                    NumberSequenceKeypoint {
                        time: 0.0,
                        value: 1.0,
                        envelope: 0.0,
                    },
                    NumberSequenceKeypoint {
                        time: 1.0,
                        value: 0.5,
                        envelope: 0.25,
                    },
                ],
            }),
        ),
        (
            "ColorSequence",
            Variant::ColorSequence(ColorSequence {
                keypoints: vec![
                    ColorSequenceKeypoint {
                        time: 0.0,
                        color: Color3Data {
                            r: 1.0,
                            g: 0.0,
                            b: 0.0,
                        },
                        envelope: 0.0,
                    },
                    ColorSequenceKeypoint {
                        time: 1.0,
                        color: Color3Data {
                            r: 0.0,
                            g: 0.5,
                            b: 1.0,
                        },
                        envelope: 0.0,
                    },
                ],
            }),
        ),
        (
            "NumberRange",
            Variant::NumberRange(NumberRange { min: 0.5, max: 4.0 }),
        ),
        (
            "Rect",
            Variant::Rect(Rect {
                min: Vector2Data { x: 0.0, y: 1.0 },
                max: Vector2Data { x: 2.5, y: 3.0 },
            }),
        ),
        (
            "PhysicalPropertiesDefault",
            Variant::PhysicalProperties(PhysicalProperties::Default),
        ),
        (
            "PhysicalPropertiesCustom",
            Variant::PhysicalProperties(PhysicalProperties::Custom {
                density: 0.7,
                friction: 0.3,
                elasticity: 0.5,
                friction_weight: 1.0,
                elasticity_weight: 2.0,
            }),
        ),
        (
            "UDim",
            Variant::UDim(UDim {
                scale: 0.5,
                offset: -10,
            }),
        ),
        (
            "UDim2",
            Variant::UDim2(UDim2 {
                x: UDim {
                    scale: 1.0,
                    offset: 5,
                },
                y: UDim {
                    scale: 0.25,
                    offset: -7,
                },
            }),
        ),
        ("Font", font),
        (
            "SecurityCapabilities",
            Variant::SecurityCapabilities(0xFFFF_FFFF),
        ),
        (
            "Content",
            Variant::Content(Content::Uri("rbxassetid://12345".to_string())),
        ),
    ];
    let target = |r: Ref| (r == Ref::new(5)).then(ref_target);
    let resolve = |t: &Target| (t.id == REF_ID).then(|| Ref::new(5));
    let out: Vec<Value> = list
        .iter()
        .map(|(name, variant)| {
            let enc = encode(variant, &target).unwrap_or_else(|| panic!("{name} encodes"));
            assert_eq!(
                decode(&enc, &resolve).as_ref(),
                Some(variant),
                "{name} round trip"
            );
            json!({"name": name, "json": enc})
        })
        .collect();
    Value::Array(out)
}

fn ops() -> Value {
    let at = |id: &str, p: &str| Target {
        id: id.to_string(),
        p: p.to_string(),
    };
    let mut props = Map::new();
    props.insert("Name".into(), json!({"t": "String", "v": "Part"}));
    props.insert("Anchored".into(), json!({"t": "Bool", "v": true}));
    let list = vec![
        Op::Set {
            at: at(REF_ID, REF_PATH),
            n: "Transparency".into(),
            v: json!({"t": "Float32", "v": 0.5}),
            c: Clock(7, "42:studio".into()),
        },
        Op::Add {
            at: at("p0123456789abcdef0123456789abcde", "Workspace.Part"),
            cls: "Part".into(),
            parent: Some(at(REF_ID, "Workspace")),
            props,
            c: Clock(8, "42:studio".into()),
        },
        Op::Del {
            at: at(REF_ID, REF_PATH),
            c: Clock(9, "43:rbx-native".into()),
        },
        Op::Src {
            at: at(REF_ID, "Workspace.Script"),
            base: text_hash("print(1)\n"),
            text: "print(2)\n".into(),
            c: Clock(10, "43:rbx-native".into()),
        },
    ];
    Value::Array(
        list.into_iter()
            .map(|op| {
                let j = serde_json::to_value(&op).expect("op serializes");
                let back: Op = serde_json::from_value(j.clone()).expect("op parses");
                assert_eq!(back, op);
                json!({"op_json": j})
            })
            .collect(),
    )
}

fn chunks() -> Value {
    let batch: String = (0..15000)
        .map(|i| char::from(b'a' + (i * 7 % 26) as u8))
        .collect();
    let cs = chunk::split("b1", &batch);
    let joined: Vec<u8> = cs
        .iter()
        .flat_map(|c| rbx_presence::crypt::b64_decode(&c.d).expect("base64"))
        .collect();
    assert_eq!(joined, batch.as_bytes());
    json!({
        "batch_json": batch,
        "id": "b1",
        "chunks": cs.iter().map(|c| serde_json::to_value(c).expect("chunk")).collect::<Vec<_>>(),
    })
}

fn hashes() -> Value {
    let texts = [
        "",
        "abc",
        "héllo wörld ✓",
        "line1\nline2\r\n",
        "日本語のテキスト 🚀",
    ];
    Value::Array(
        texts
            .iter()
            .map(|t| json!({"text": t, "hash": text_hash(t)}))
            .collect(),
    )
}

fn merges() -> Value {
    let cases = [
        ("a\nb\nc\n", "a\nB\nc\n", "a\nb\nc\n"),
        ("a\nb\nc\nd\ne\n", "A\nb\nc\nd\ne\n", "a\nb\nc\nd\nE\n"),
        ("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n"),
        ("a\nb\n", "a\nX\n", "a\nX\n"),
    ];
    Value::Array(
        cases
            .iter()
            .map(|(base, ours, theirs)| {
                let m = merge3(base, ours, theirs);
                json!({"base": base, "ours": ours, "theirs": theirs,
                       "text": m.text, "conflicted": m.conflicted})
            })
            .collect(),
    )
}

fn path_ids() -> Value {
    let cases = [
        ("Workspace", 0usize),
        ("Workspace.Part", 0),
        ("Workspace.Part", 1),
        ("ReplicatedStorage.Módulo", 2),
    ];
    Value::Array(
        cases
            .iter()
            .map(|(p, n)| json!({"path": p, "n": n, "id": path_id(p, *n)}))
            .collect(),
    )
}

fn unique_hexes() -> Value {
    let cases = [
        (1u32, 2u32, 3i64),
        (0xdead_beef, 0x1234_5678, 0x0123_4567_89ab_cdef),
        (7, 8, -2),
        (0, 0, i64::MIN),
    ];
    Value::Array(
        cases
            .iter()
            .map(|&(index, time, random)| {
                let hex = unique_hex(&UniqueId {
                    index,
                    time,
                    random,
                })
                .expect("non-zero id");
                json!({"index": index, "time": time, "random": random, "hex": hex})
            })
            .collect(),
    )
}

fn build() -> Value {
    json!({
        "values": values(),
        "ops": ops(),
        "chunks": chunks(),
        "text_hash": hashes(),
        "merge3": merges(),
        "path_id": path_ids(),
        "unique_hex": unique_hexes(),
    })
}

#[test]
fn golden_vectors_match_fixture() {
    let built = build();
    let path = fixture_path();
    if std::env::var("RBX_PRESENCE_REGEN").as_deref() == Ok("1") {
        let mut text = serde_json::to_string_pretty(&built).expect("pretty json");
        text.push('\n');
        std::fs::write(&path, text).expect("write fixture");
        return;
    }
    let text = std::fs::read_to_string(&path).expect("read fixture");
    let fixture: Value = serde_json::from_str(&text).expect("fixture is JSON");
    assert_eq!(
        fixture, built,
        "fixture is stale; regenerate with RBX_PRESENCE_REGEN=1"
    );
}
