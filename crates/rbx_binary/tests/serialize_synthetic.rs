//! Round-trips a hand-built DOM covering every `Variant` kind the serializer supports,
//! one instance per kind so no class needs a property present on some instances but
//! not others (see `serialize::SerializeError::InconsistentProperty`).

use rbx_binary::{deserialize, serialize};
use rbx_dom::{
    Axes, CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Content, Faces, Font,
    FontStyle, NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, Ref,
    UDim, UDim2, UniqueId, Variant, Vector2Data, Vector3Data, WeakDom,
};

fn set(dom: &mut WeakDom, class: &str, name: &str, value: Variant) -> Ref {
    let referent = dom.new_instance(class, name, None);
    dom.set_property(referent, "Value", value).unwrap();
    referent
}

fn build() -> WeakDom {
    let mut dom = WeakDom::new();

    set(
        &mut dom,
        "T_String",
        "s",
        Variant::String("hello".to_owned()),
    );
    set(&mut dom, "T_Bool", "b", Variant::Bool(true));
    set(&mut dom, "T_Int32", "i32", Variant::Int32(-42));
    set(&mut dom, "T_Int64", "i64", Variant::Int64(i64::MIN));
    set(&mut dom, "T_Float32", "f32", Variant::Float32(1.5));
    set(&mut dom, "T_Float64", "f64", Variant::Float64(-2.25));
    set(&mut dom, "T_BrickColor", "bc", Variant::BrickColor(194));
    set(
        &mut dom,
        "T_Color3",
        "c3",
        Variant::Color3(Color3Data {
            r: 0.1,
            g: 0.2,
            b: 0.3,
        }),
    );
    set(
        &mut dom,
        "T_Color3uint8",
        "c3u",
        Variant::Color3uint8 { r: 1, g: 2, b: 3 },
    );
    set(
        &mut dom,
        "T_Vector2",
        "v2",
        Variant::Vector2(Vector2Data { x: -1.5, y: 2.5 }),
    );
    set(
        &mut dom,
        "T_Vector3",
        "v3",
        Variant::Vector3(Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        }),
    );
    set(
        &mut dom,
        "T_Vector3int16",
        "v3i",
        Variant::Vector3int16 { x: -1, y: 2, z: -3 },
    );
    set(
        &mut dom,
        "T_Ray",
        "ray",
        Variant::Ray {
            origin: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            direction: Vector3Data {
                x: 0.0,
                y: -1.0,
                z: 0.0,
            },
        },
    );
    set(
        &mut dom,
        "T_Faces",
        "faces",
        Variant::Faces(Faces {
            front: true,
            bottom: false,
            left: true,
            back: false,
            top: true,
            right: false,
        }),
    );
    set(
        &mut dom,
        "T_Axes",
        "axes",
        Variant::Axes(Axes {
            x: true,
            y: false,
            z: true,
        }),
    );
    set(
        &mut dom,
        "T_CFrame",
        "cf",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            rotation: [0.9, 0.1, 0.0, -0.1, 0.9, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    set(
        &mut dom,
        "T_OptionalCFrame",
        "ocf_some",
        Variant::OptionalCFrame(Some(CFrameData {
            position: Vector3Data {
                x: 4.0,
                y: 5.0,
                z: 6.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        })),
    );
    set(
        &mut dom,
        "T_OptionalCFrameAbsent",
        "ocf_none",
        Variant::OptionalCFrame(None),
    );
    set(&mut dom, "T_Enum", "e", Variant::Enum(3));

    let target = dom.new_instance("Folder", "Target", None);
    set(&mut dom, "T_Ref", "r", Variant::Ref(target));

    set(
        &mut dom,
        "T_NumberSequence",
        "ns",
        Variant::NumberSequence(NumberSequence {
            keypoints: vec![
                NumberSequenceKeypoint {
                    time: 0.0,
                    value: 0.0,
                    envelope: 0.0,
                },
                NumberSequenceKeypoint {
                    time: 1.0,
                    value: 1.0,
                    envelope: 0.0,
                },
            ],
        }),
    );
    set(
        &mut dom,
        "T_ColorSequence",
        "cs",
        Variant::ColorSequence(ColorSequence {
            keypoints: vec![ColorSequenceKeypoint {
                time: 0.0,
                color: Color3Data {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                },
                envelope: 0.0,
            }],
        }),
    );
    set(
        &mut dom,
        "T_NumberRange",
        "nr",
        Variant::NumberRange(NumberRange { min: 0.5, max: 1.5 }),
    );
    set(
        &mut dom,
        "T_Rect",
        "rect",
        Variant::Rect(Rect {
            min: Vector2Data { x: 0.0, y: 0.0 },
            max: Vector2Data { x: 10.0, y: 20.0 },
        }),
    );
    set(
        &mut dom,
        "T_PhysicalPropertiesDefault",
        "pp_default",
        Variant::PhysicalProperties(PhysicalProperties::Default),
    );
    set(
        &mut dom,
        "T_PhysicalPropertiesCustom",
        "pp_custom",
        Variant::PhysicalProperties(PhysicalProperties::Custom {
            density: 0.7,
            friction: 0.3,
            elasticity: 0.5,
            friction_weight: 1.0,
            elasticity_weight: 2.0,
        }),
    );
    set(&mut dom, "T_SharedString", "ss", Variant::SharedString(7));
    set(
        &mut dom,
        "T_UDim",
        "udim",
        Variant::UDim(UDim {
            scale: 0.5,
            offset: -3,
        }),
    );
    set(
        &mut dom,
        "T_UDim2",
        "udim2",
        Variant::UDim2(UDim2 {
            x: UDim {
                scale: 0.0,
                offset: 100,
            },
            y: UDim {
                scale: 1.0,
                offset: -100,
            },
        }),
    );
    set(
        &mut dom,
        "T_UniqueId",
        "uid",
        Variant::UniqueId(UniqueId {
            index: 2,
            time: 0x09e5_50b2,
            random: 0x0038_2dbe_99a6_3c35,
        }),
    );
    set(
        &mut dom,
        "T_Font",
        "font",
        Variant::Font(Font {
            family: "rbxasset://fonts/families/BuilderSans.json".to_owned(),
            weight: 700,
            style: FontStyle::Normal,
            cached_face_id: Some("rbxasset://fonts/BuilderSans-Bold.otf".to_owned()),
        }),
    );
    set(
        &mut dom,
        "T_SecurityCapabilities",
        "sec",
        Variant::SecurityCapabilities(0x2A),
    );
    set(
        &mut dom,
        "T_ContentNone",
        "content_none",
        Variant::Content(Content::None),
    );
    set(
        &mut dom,
        "T_ContentUri",
        "content_uri",
        Variant::Content(Content::Uri("rbxassetid://123".to_owned())),
    );
    set(
        &mut dom,
        "T_ContentObject",
        "content_object",
        Variant::Content(Content::Object(target)),
    );
    set(
        &mut dom,
        "T_Unknown",
        "unk",
        Variant::Unknown {
            type_id: 0x1D,
            raw: vec![0xDE, 0xAD, 0xBE, 0xEF],
        },
    );

    dom
}

#[test]
fn every_supported_variant_kind_round_trips() {
    let before = build();
    let bytes = serialize(&before).unwrap();
    let after = deserialize(&bytes).unwrap();

    let mut checked = 0;
    for &referent in before.root_refs() {
        let original = before.get(referent).unwrap();
        let round_tripped = after.get(referent).unwrap_or_else(|| {
            panic!(
                "{referent:?} ({}) missing after round-trip",
                original.class()
            )
        });

        assert_eq!(original.class(), round_tripped.class());
        assert_eq!(original.name(), round_tripped.name());
        assert_eq!(
            original.properties(),
            round_tripped.properties(),
            "{} ({:?}) properties changed",
            original.class(),
            original.name()
        );
        checked += 1;
    }

    assert_eq!(checked, before.root_refs().len());
    // One instance per supported `Variant` kind (33), an OptionalCFrame::None on a
    // separate class to avoid a same-class kind clash, and the plain "Target" Folder
    // used by Ref/Content::Object.
    assert_eq!(checked, 37);
}
