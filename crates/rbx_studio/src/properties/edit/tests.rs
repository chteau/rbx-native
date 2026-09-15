use rbx_dom::{
    CFrameData, Color3Data, Instance, NumberRange, UDim, UDim2, Vector2Data, Vector3Data,
};

use super::*;

fn db() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

fn part_ref() -> Ref {
    Ref::new(1)
}

/// One `Part` named `Baseplate`, carrying whatever properties the test needs.
fn dom_with(values: &[(&str, Variant)]) -> WeakDom {
    let mut dom = WeakDom::new();
    let mut part = Instance::new(part_ref(), "Part", "Baseplate");
    for (name, value) in values {
        part.properties_mut()
            .insert((*name).to_owned(), value.clone());
    }
    dom.insert(part);
    dom
}

// --- edit_text: which types are editable at all -----------------------

#[test]
fn scalar_and_composite_types_have_edit_text() {
    assert_eq!(edit_text(&Variant::Bool(true)), Some("true".to_owned()));
    assert_eq!(edit_text(&Variant::Int32(5)), Some("5".to_owned()));
    assert_eq!(
        edit_text(&Variant::String("hi".into())),
        Some("hi".to_owned())
    );
    assert_eq!(
        edit_text(&Variant::Vector3(Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0
        })),
        Some("1, 2, 3".to_owned())
    );
    assert_eq!(
        edit_text(&Variant::Color3uint8 {
            r: 10,
            g: 20,
            b: 30
        }),
        Some("10, 20, 30".to_owned())
    );
}

// Anything `parse` cannot round-trip must stay read-only text in the panel.
#[test]
fn unsupported_types_have_no_edit_text() {
    assert_eq!(edit_text(&Variant::Ref(Ref::new(1))), None);
    assert_eq!(edit_text(&Variant::Vector3int16 { x: 1, y: 2, z: 3 }), None);
    assert_eq!(edit_text(&Variant::OptionalCFrame(None)), None);
}

// A `CFrame`'s rotation has no accepted syntax, so only the position shows.
#[test]
fn cframe_edit_text_is_position_only() {
    let frame = Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    });
    assert_eq!(edit_text(&frame), Some("4, 5, 6".to_owned()));
}

// --- parse: one case per supported type ---------------------------------

fn parse_as(current: &Variant, text: &str) -> Result<Variant, String> {
    parse(current, &db(), "Part", "Anchored", text)
}

#[test]
fn bool_accepts_true_and_false_case_insensitively() {
    assert_eq!(
        parse_as(&Variant::Bool(false), "true"),
        Ok(Variant::Bool(true))
    );
    assert_eq!(
        parse_as(&Variant::Bool(false), "FALSE"),
        Ok(Variant::Bool(false))
    );
    assert_eq!(
        parse_as(&Variant::Bool(false), "  True  "),
        Ok(Variant::Bool(true))
    );
    assert!(parse_as(&Variant::Bool(false), "yes").is_err());
}

#[test]
fn numbers_parse_and_reject_junk() {
    assert_eq!(parse_as(&Variant::Int32(0), " 42 "), Ok(Variant::Int32(42)));
    assert_eq!(parse_as(&Variant::Int64(0), "-7"), Ok(Variant::Int64(-7)));
    assert_eq!(
        parse_as(&Variant::Float32(0.0), "1.5"),
        Ok(Variant::Float32(1.5))
    );
    assert_eq!(
        parse_as(&Variant::Float64(0.0), "1.5"),
        Ok(Variant::Float64(1.5))
    );
    assert!(parse_as(&Variant::Int32(0), "not a number").is_err());
    assert!(parse_as(&Variant::Float32(0.0), "").is_err());
}

// BrickColor has no bundled RGB palette (see `edit_text`'s doc comment), so
// it edits as a plain palette-index number, the same shape as an Int32.
#[test]
fn brick_color_edits_as_its_raw_palette_index() {
    assert_eq!(edit_text(&Variant::BrickColor(194)), Some("194".to_owned()));
    assert_eq!(
        parse_as(&Variant::BrickColor(0), " 21 "),
        Ok(Variant::BrickColor(21))
    );
    assert!(parse_as(&Variant::BrickColor(0), "not a number").is_err());
}

#[test]
fn string_is_taken_verbatim() {
    assert_eq!(
        parse_as(&Variant::String(String::new()), "  hello world  "),
        Ok(Variant::String("hello world".to_owned()))
    );
}

#[test]
fn vector2_and_vector3_split_on_commas_and_ignore_brackets() {
    let zero3 = Variant::Vector3(Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    });
    assert_eq!(
        parse_as(&zero3, "1, 2, 3"),
        Ok(Variant::Vector3(Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0
        }))
    );
    assert_eq!(
        parse_as(&zero3, "(1, 2, 3)"),
        Ok(Variant::Vector3(Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0
        }))
    );
    assert!(parse_as(&zero3, "1, 2").is_err());

    let zero2 = Variant::Vector2(Vector2Data { x: 0.0, y: 0.0 });
    assert_eq!(
        parse_as(&zero2, "4, 5"),
        Ok(Variant::Vector2(Vector2Data { x: 4.0, y: 5.0 }))
    );
}

#[test]
fn color3_infers_0_255_scale_when_a_channel_exceeds_1() {
    let black = Variant::Color3(Color3Data {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    });
    assert_eq!(
        parse_as(&black, "255, 0, 0"),
        Ok(Variant::Color3(Color3Data {
            r: 1.0,
            g: 0.0,
            b: 0.0
        }))
    );
    assert_eq!(
        parse_as(&black, "0.5, 0.5, 0.5"),
        Ok(Variant::Color3(Color3Data {
            r: 0.5,
            g: 0.5,
            b: 0.5
        }))
    );
}

#[test]
fn color3uint8_rounds_and_clamps() {
    let value = Variant::Color3uint8 { r: 0, g: 0, b: 0 };
    assert_eq!(
        parse_as(&value, "254.6, 0, 300"),
        Ok(Variant::Color3uint8 {
            r: 255,
            g: 0,
            b: 255
        })
    );
    assert!(parse_as(&value, "1, 2").is_err());
}

#[test]
fn enum_accepts_item_names_case_insensitively_and_numbers() {
    let material = Variant::Enum(0);
    let database = db();
    assert_eq!(
        parse(&material, &database, "Part", "Material", "smoothplastic"),
        Ok(Variant::Enum(272))
    );
    assert_eq!(
        parse(&material, &database, "Part", "Material", "272"),
        Ok(Variant::Enum(272))
    );
    assert!(parse(&material, &database, "Part", "Material", "NotAMaterial").is_err());
}

#[test]
fn udim_and_udim2_read_scale_then_offset_pairs() {
    let udim = Variant::UDim(UDim {
        scale: 0.0,
        offset: 0,
    });
    assert_eq!(
        parse_as(&udim, "0.5, 10"),
        Ok(Variant::UDim(UDim {
            scale: 0.5,
            offset: 10
        }))
    );

    let udim2 = Variant::UDim2(UDim2 {
        x: UDim {
            scale: 0.0,
            offset: 0,
        },
        y: UDim {
            scale: 0.0,
            offset: 0,
        },
    });
    let expected = Variant::UDim2(UDim2 {
        x: UDim {
            scale: 0.0,
            offset: 10,
        },
        y: UDim {
            scale: 1.0,
            offset: 20,
        },
    });
    assert_eq!(parse_as(&udim2, "0, 10, 1, 20"), Ok(expected.clone()));
    assert_eq!(parse_as(&udim2, "{0, 10}, {1, 20}"), Ok(expected));
}

#[test]
fn cframe_keeps_its_rotation_and_only_replaces_the_position() {
    let rotation = [2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0];
    let current = Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation,
    });

    assert_eq!(
        parse_as(&current, "7, 8, 9"),
        Ok(Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 7.0,
                y: 8.0,
                z: 9.0
            },
            rotation,
        }))
    );
    assert!(parse_as(&current, "7, 8").is_err());
}

#[test]
fn number_range_reads_min_then_max() {
    let range = Variant::NumberRange(NumberRange { min: 0.0, max: 0.0 });
    assert_eq!(
        parse_as(&range, "1, 5"),
        Ok(Variant::NumberRange(NumberRange { min: 1.0, max: 5.0 }))
    );
    assert!(parse_as(&range, "1, 5, 9").is_err());
}

#[test]
fn a_type_edit_text_never_approved_is_rejected_defensively() {
    let value = Variant::Ref(Ref::new(1));
    assert!(parse_as(&value, "1").is_err());
}

// --- commit: the DOM take/put-back path ---------------------------------

#[test]
fn a_successful_commit_writes_the_value_and_returns_the_previous_one() {
    let mut dom = dom_with(&[("Transparency", Variant::Float32(0.0))]);

    let previous = commit(&mut dom, &db(), part_ref(), "Transparency", "0.75")
        .expect("a valid number should commit");

    assert_eq!(previous, Some(Variant::Float32(0.0)));
    assert_eq!(
        dom.get(part_ref())
            .unwrap()
            .properties()
            .get("Transparency"),
        Some(&Variant::Float32(0.75))
    );
}

#[test]
fn a_failed_commit_leaves_the_dom_untouched() {
    let mut dom = dom_with(&[("Transparency", Variant::Float32(0.25))]);

    let result = commit(&mut dom, &db(), part_ref(), "Transparency", "not a number");

    assert!(result.is_err());
    assert_eq!(
        dom.get(part_ref())
            .unwrap()
            .properties()
            .get("Transparency"),
        Some(&Variant::Float32(0.25))
    );
}

#[test]
fn committing_name_renames_the_instance_rather_than_setting_a_property() {
    let mut dom = dom_with(&[]);

    let previous = commit(&mut dom, &db(), part_ref(), NAME_PROPERTY, "Renamed")
        .expect("any text is a valid name");

    assert_eq!(previous, Some(Variant::String("Baseplate".to_owned())));
    assert_eq!(dom.get(part_ref()).unwrap().name(), "Renamed");
}

#[test]
fn committing_a_property_the_instance_never_had_is_an_error() {
    let mut dom = dom_with(&[]);

    let result = commit(&mut dom, &db(), part_ref(), "Transparency", "0.5");

    assert!(result.is_err());
    assert!(dom
        .get(part_ref())
        .unwrap()
        .properties()
        .get("Transparency")
        .is_none());
}

#[test]
fn committing_to_an_unknown_referent_is_an_error() {
    let mut dom = dom_with(&[("Transparency", Variant::Float32(0.0))]);

    let result = commit(&mut dom, &db(), Ref::new(999), "Transparency", "0.5");

    assert!(result.is_err());
}

/// What the viewport's Rotate drag writes through: the rotation alone, leaving
/// the part standing where it was. Nine terms row by row, the order Roblox's
/// own `CFrame.new(x, y, z, R00 … R22)` takes them in.
#[test]
fn cframe_takes_nine_numbers_as_a_rotation_and_keeps_the_position() {
    let position = Vector3Data {
        x: 7.0,
        y: 8.0,
        z: 9.0,
    };
    let current = Variant::CFrame(CFrameData {
        position,
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    });
    // A quarter turn about Y.
    let turned = [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0];

    assert_eq!(
        parse_as(&current, "0, 0, 1, 0, 1, 0, -1, 0, 0"),
        Ok(Variant::CFrame(CFrameData {
            position,
            rotation: turned,
        }))
    );
}

#[test]
fn cframe_takes_twelve_numbers_as_a_whole_placement() {
    let current = Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    });

    assert_eq!(
        parse_as(&current, "1, 2, 3, 0, 0, 1, 0, 1, 0, -1, 0, 0"),
        Ok(Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 1.0,
                y: 2.0,
                z: 3.0
            },
            rotation: [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0],
        }))
    );
}

#[test]
fn a_cframe_of_no_recognised_length_is_rejected() {
    let current = Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    });

    assert!(parse_as(&current, "1, 2").is_err());
    assert!(parse_as(&current, "1, 2, 3, 4").is_err());
    assert!(parse_as(&current, "0, 0, 1, 0, 1, 0, -1, 0").is_err());
}
