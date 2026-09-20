use rbx_dom::{
    Axes, CFrameData, Color3Data, Content, Faces, Font, FontStyle, Instance, NumberRange,
    NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, UDim, UDim2, UniqueId,
    Vector2Data, Vector3Data,
};

use super::*;

fn part() -> Ref {
    Ref::new(2)
}

fn workspace() -> Ref {
    Ref::new(1)
}

/// A tree and the panel reading it, paired the way `Shell` holds them: the
/// panel borrows the DOM on every call, so a test asks the pair rather than
/// the panel alone.
struct Fixture {
    dom: WeakDom,
    properties: Properties,
}

impl Fixture {
    fn rows(&self, reference: Ref) -> Vec<PropertyRow> {
        self.properties.rows(&self.dom, reference, None)
    }

    fn rows_with_folder_color(&self, reference: Ref, color: (u8, u8, u8)) -> Vec<PropertyRow> {
        self.properties.rows(&self.dom, reference, Some(color))
    }

    fn title(&self, reference: Ref) -> Option<String> {
        self.properties.title(&self.dom, reference)
    }

    fn rows_matching(&self, reference: Ref, filter: &str) -> Vec<PropertyRow> {
        self.properties
            .rows_matching(&self.dom, reference, filter, None)
    }
}

/// A Workspace holding one Part carrying `properties`, wrapped with the real
/// reflection dump so enum names resolve the way they do in the editor.
fn properties(values: &[(&str, Variant)]) -> Fixture {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(workspace(), "Workspace", "Workspace"));
    let mut instance = Instance::new(part(), "Part", "Baseplate");
    for (name, value) in values {
        instance
            .properties_mut()
            .insert((*name).to_owned(), value.clone());
    }
    dom.insert(instance);
    dom.set_parent(part(), Some(workspace()));

    Fixture {
        dom,
        properties: Properties::new(ReflectionDatabase::embedded()),
    }
}

/// The row for `name` specifically: `rows()` also carries a synthesized
/// `Name` row (see `Properties::rows`), which is irrelevant to a test only
/// checking one property's formatting.
fn formatted(name: &str, value: Variant) -> String {
    properties(&[(name, value)])
        .rows(part())
        .into_iter()
        .find(|row| row.name == name)
        .expect("the row for the property under test")
        .value
}

fn vector3(x: f32, y: f32, z: f32) -> Vector3Data {
    Vector3Data { x, y, z }
}

/// The row for `name`'s [`EditKind`].
fn edit_kind(name: &str, value: Variant) -> Option<EditKind> {
    properties(&[(name, value)])
        .rows(part())
        .into_iter()
        .find(|row| row.name == name)
        .expect("the row for the property under test")
        .edit
}

/// The row for `name`'s resolved category.
fn category(name: &str, value: Variant) -> String {
    properties(&[(name, value)])
        .rows(part())
        .into_iter()
        .find(|row| row.name == name)
        .expect("the row for the property under test")
        .category
}

#[test]
fn the_title_is_the_class_and_the_quoted_name() {
    let properties = properties(&[]);

    assert_eq!(
        properties.title(part()).as_deref(),
        Some("Part \"Baseplate\"")
    );
    assert_eq!(properties.title(Ref::new(99)), None);
}

#[test]
fn rows_come_sorted_by_name() {
    let rows = properties(&[
        ("Transparency", Variant::Float32(0.5)),
        ("Anchored", Variant::Bool(true)),
        ("Name", Variant::String("Baseplate".into())),
    ])
    .rows(part());

    let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(names, ["Anchored", "Name", "Transparency"]);
}

#[test]
fn a_missing_instance_has_no_rows() {
    assert!(properties(&[]).rows(Ref::new(99)).is_empty());
}

#[test]
fn a_folder_gets_the_synthetic_explorer_colour_row() {
    let rows = instance_of("Folder", &[]).rows(part());
    let row = rows
        .iter()
        .find(|row| row.name == edit::FOLDER_COLOR_PROPERTY)
        .expect("Explorer Colour row");
    // Untagged: seeded white, the same "no tint" the Explorer itself shows.
    assert_eq!(
        row.edit,
        Some(EditKind::Color {
            r: 255,
            g: 255,
            b: 255
        })
    );
}

#[test]
fn a_non_folder_never_gets_the_synthetic_explorer_colour_row() {
    for class in ["Part", "Model", "Workspace", "Script"] {
        let rows = instance_of(class, &[]).rows(part());
        assert!(
            rows.iter()
                .all(|row| row.name != edit::FOLDER_COLOR_PROPERTY),
            "{class} should not carry an Explorer Colour row"
        );
    }
}

#[test]
fn a_folders_explorer_colour_row_seeds_from_the_passed_in_tag() {
    let rows = instance_of("Folder", &[]).rows_with_folder_color(part(), (10, 20, 30));
    let row = rows
        .iter()
        .find(|row| row.name == edit::FOLDER_COLOR_PROPERTY)
        .expect("Explorer Colour row");
    assert_eq!(
        row.edit,
        Some(EditKind::Color {
            r: 10,
            g: 20,
            b: 30
        })
    );
    assert_eq!(row.value, "(10, 20, 30)");
}

#[test]
fn scalars_print_plainly_and_strings_quoted() {
    assert_eq!(formatted("Anchored", Variant::Bool(true)), "true");
    assert_eq!(formatted("Count", Variant::Int32(-3)), "-3");
    assert_eq!(formatted("Big", Variant::Int64(1 << 40)), "1099511627776");
    assert_eq!(formatted("Reflectance", Variant::Float32(0.0)), "0");
    // Not `BackParamA`: that name is real on `BasePart` and, as of the
    // Hidden/read-only filtering below, tagged `Hidden` in the dump — this
    // assertion only cares about plain `Float32` formatting.
    assert_eq!(formatted("BevelAmount", Variant::Float32(-0.5)), "-0.5");
    assert_eq!(formatted("Precise", Variant::Float64(2.5)), "2.5");
    assert_eq!(
        formatted("CollisionGroup", Variant::String("Default".into())),
        "\"Default\""
    );
}

#[test]
fn long_and_binary_values_show_their_size_only() {
    let long = "x".repeat(MAX_STRING_LEN + 1);
    assert_eq!(
        formatted("Source", Variant::String(long)),
        format!("<{} bytes>", MAX_STRING_LEN + 1)
    );
    assert_eq!(
        formatted(
            "Mystery",
            Variant::Unknown {
                type_id: 0x7f,
                raw: vec![0; 12],
            }
        ),
        "<12 bytes>"
    );
    // Exactly at the limit still reads as text.
    let fits = "y".repeat(MAX_STRING_LEN);
    assert_eq!(
        formatted("Fits", Variant::String(fits.clone())),
        format!("{fits:?}")
    );
}

#[test]
fn vectors_are_parenthesized_triplets() {
    assert_eq!(
        formatted("Size", Variant::Vector3(vector3(512.0, 20.0, 512.0))),
        "(512, 20, 512)"
    );
    assert_eq!(
        formatted("Offset", Variant::Vector2(Vector2Data { x: 1.5, y: -2.0 })),
        "(1.5, -2)"
    );
    assert_eq!(
        formatted("Cell", Variant::Vector3int16 { x: 1, y: 2, z: 3 }),
        "(1, 2, 3)"
    );
}

#[test]
fn a_cframe_shows_position_then_rotation() {
    let frame = CFrameData {
        position: vector3(0.0, -8.0, 0.0),
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };

    assert_eq!(
        formatted("CFrame", Variant::CFrame(frame)),
        "pos=(0, -8, 0) rot=[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]"
    );
    assert_eq!(
        formatted("PivotOffset", Variant::OptionalCFrame(Some(frame))),
        "pos=(0, -8, 0) rot=[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]"
    );
    assert_eq!(
        formatted("PivotOffset", Variant::OptionalCFrame(None)),
        "none"
    );
}

#[test]
fn colors_are_byte_triplets_whichever_way_they_were_stored() {
    let color = Color3Data {
        r: 0.0,
        g: 0.5,
        b: 1.0,
    };

    assert_eq!(formatted("Color", Variant::Color3(color)), "(0, 128, 255)");
    assert_eq!(
        formatted(
            "Color3uint8",
            Variant::Color3uint8 {
                r: 91,
                g: 91,
                b: 91
            }
        ),
        "(91, 91, 91)"
    );
    assert_eq!(
        formatted("BrickColor", Variant::BrickColor(194)),
        "BrickColor(194)"
    );
}

#[test]
fn enums_resolve_to_their_name_through_the_reflection_dump() {
    assert_eq!(formatted("Material", Variant::Enum(256)), "256 (Plastic)");
    assert_eq!(formatted("BackSurface", Variant::Enum(0)), "0 (Smooth)");
}

#[test]
fn an_unknown_enum_stays_a_bare_ordinal() {
    // A value the dump has no name for, and a property it has never heard of.
    assert_eq!(formatted("Material", Variant::Enum(999_999)), "999999");
    assert_eq!(formatted("MadeUp", Variant::Enum(7)), "7");
}

#[test]
fn refs_read_as_the_target_name() {
    assert_eq!(formatted("Parent", Variant::Ref(workspace())), "Workspace");
    assert_eq!(formatted("Dangling", Variant::Ref(Ref::new(99))), "nil");
    assert_eq!(
        formatted("Mesh", Variant::Content(Content::Object(workspace()))),
        "Workspace"
    );
}

#[test]
fn content_variants_keep_their_kind_visible() {
    assert_eq!(
        formatted("Texture", Variant::Content(Content::None)),
        "Content(none)"
    );
    assert_eq!(
        formatted(
            "Texture",
            Variant::Content(Content::Uri("rbxassetid://1".into()))
        ),
        "Content(\"rbxassetid://1\")"
    );
}

#[test]
fn bit_flags_list_the_set_names_in_wire_order() {
    assert_eq!(
        formatted("Faces", Variant::Faces(Faces::from_bits(0b01_0001))),
        "Faces(Front|Top)"
    );
    assert_eq!(
        formatted("Faces", Variant::Faces(Faces::default())),
        "Faces()"
    );
    assert_eq!(
        formatted("Axes", Variant::Axes(Axes::from_bits(0b101))),
        "Axes(X|Z)"
    );
}

#[test]
fn compound_values_match_the_text_dump() {
    assert_eq!(
        formatted(
            "Ray",
            Variant::Ray {
                origin: vector3(0.0, 1.0, 0.0),
                direction: vector3(0.0, -1.0, 0.0),
            }
        ),
        "Ray { origin: (0, 1, 0), direction: (0, -1, 0) }"
    );
    assert_eq!(
        formatted(
            "Range",
            Variant::NumberRange(NumberRange { min: 1.0, max: 2.5 })
        ),
        "[1, 2.5]"
    );
    assert_eq!(
        formatted(
            "Rect",
            Variant::Rect(rbx_dom::Rect {
                min: Vector2Data { x: 0.0, y: 0.0 },
                max: Vector2Data { x: 4.0, y: 2.0 },
            })
        ),
        "{(0, 0), (4, 2)}"
    );
    assert_eq!(
        formatted(
            "Curve",
            Variant::NumberSequence(NumberSequence {
                keypoints: vec![NumberSequenceKeypoint {
                    time: 0.0,
                    value: 1.0,
                    envelope: 0.0,
                }],
            })
        ),
        "NumberSequence[0: 1 ±0]"
    );
    assert_eq!(
        formatted(
            "Physics",
            Variant::PhysicalProperties(PhysicalProperties::Default)
        ),
        "Default"
    );
}

#[test]
fn ui_dimensions_use_roblox_brace_notation() {
    assert_eq!(
        formatted(
            "Size",
            Variant::UDim(UDim {
                scale: 0.5,
                offset: 10
            })
        ),
        "{0.5, 10}"
    );
    assert_eq!(
        // Not `Position`: that name is real on `BasePart` and tagged
        // `Hidden` in the dump, so the row wouldn't exist to format — this
        // assertion only cares about plain `UDim2` formatting.
        formatted(
            "GuiPosition",
            Variant::UDim2(UDim2 {
                x: UDim {
                    scale: 0.0,
                    offset: 4
                },
                y: UDim {
                    scale: 1.0,
                    offset: -4
                },
            })
        ),
        "{{0, 4}, {1, -4}}"
    );
}

#[test]
fn identity_and_asset_values_keep_the_dump_spelling() {
    assert_eq!(
        formatted(
            "UniqueId",
            Variant::UniqueId(UniqueId {
                index: 1,
                time: 2,
                random: 3,
            })
        ),
        "00000001000000020000000000000003"
    );
    assert_eq!(
        formatted(
            "FontFace",
            Variant::Font(Font {
                family: "rbxasset://fonts/families/Arial.json".into(),
                weight: 400,
                style: FontStyle::Normal,
                cached_face_id: None,
            })
        ),
        "Font { family: \"rbxasset://fonts/families/Arial.json\", weight: 400, style: Normal }"
    );
    assert_eq!(
        formatted("Capabilities", Variant::SecurityCapabilities(0x10)),
        "SecurityCapabilities(0x10)"
    );
    assert_eq!(
        formatted("Tags", Variant::SharedString(3)),
        "SharedString(3)"
    );
}

#[test]
fn the_filter_is_a_case_insensitive_substring_of_the_name() {
    assert!(matches("CanCollide", "collide"));
    assert!(matches("CanCollide", "CANC"));
    assert!(matches("CanCollide", ""));
    assert!(matches("CanCollide", "  "));
    assert!(!matches("CanCollide", "anchor"));
    assert!(!matches("CanCollide", "Collide Can"));
}

#[test]
fn filtered_rows_keep_only_matching_names_in_order() {
    let properties = properties(&[
        ("CanTouch", Variant::Bool(true)),
        ("Anchored", Variant::Bool(true)),
        ("CanCollide", Variant::Bool(false)),
    ]);

    let names = |filter: &str| -> Vec<String> {
        properties
            .rows_matching(part(), filter)
            .into_iter()
            .map(|row| row.name)
            .collect()
    };
    assert_eq!(names("can"), ["CanCollide", "CanTouch"]);
    // `Name` is synthesized for every instance (see `Properties::rows`), not
    // just the properties this fixture inserted.
    assert_eq!(names(""), ["Anchored", "CanCollide", "CanTouch", "Name"]);
    assert!(names("zzz").is_empty());
}

#[test]
fn category_comes_from_the_reflection_dump() {
    assert_eq!(category("Anchored", Variant::Bool(true)), "Part");
    assert_eq!(category("CanCollide", Variant::Bool(true)), "Collision");
    assert_eq!(
        category(
            "Color",
            Variant::Color3(Color3Data {
                r: 0.0,
                g: 0.0,
                b: 0.0
            })
        ),
        "Appearance"
    );
}

#[test]
fn an_unreflected_property_falls_back_to_the_other_category() {
    assert_eq!(category("MadeUp", Variant::Bool(true)), "Other");
}

#[test]
fn bool_edits_as_a_checkbox() {
    assert_eq!(
        edit_kind("Anchored", Variant::Bool(true)),
        Some(EditKind::Bool(true))
    );
}

#[test]
fn color3_edits_as_0_255_channels_matching_the_read_only_display() {
    let color = Color3Data {
        r: 0.0,
        g: 0.5,
        b: 1.0,
    };
    assert_eq!(
        // Not `Color`: that name is real on `BasePart` and, per the dump's
        // Serialization, not saveable — read-only there now, which this
        // assertion isn't testing.
        edit_kind("TintColor", Variant::Color3(color)),
        Some(EditKind::Color {
            r: 0,
            g: 128,
            b: 255
        })
    );
}

#[test]
fn color3uint8_edits_its_stored_bytes_directly() {
    assert_eq!(
        edit_kind(
            "Color3uint8Test",
            Variant::Color3uint8 {
                r: 10,
                g: 20,
                b: 30
            }
        ),
        Some(EditKind::Color {
            r: 10,
            g: 20,
            b: 30
        })
    );
}

#[test]
fn brick_color_stays_a_text_field_with_no_bundled_palette() {
    assert_eq!(
        // Not `BrickColor`: that name is real on `BasePart` and, per the
        // dump's Serialization, not saveable — read-only there now, which
        // this assertion isn't testing.
        edit_kind("LegacyBrickColor", Variant::BrickColor(194)),
        Some(EditKind::Text("194".to_owned()))
    );
}

#[test]
fn enum_edits_as_a_dropdown_of_every_resolved_member() {
    let Some(EditKind::Enum { current, items }) = edit_kind("Material", Variant::Enum(256)) else {
        panic!("Material should resolve to a dropdown");
    };
    assert_eq!(current, "Plastic");
    assert!(items.iter().any(|item| item == "Plastic"));
    assert!(items.len() > 1);
}

#[test]
fn an_unresolved_enum_falls_back_to_its_raw_ordinal_as_text() {
    assert_eq!(
        edit_kind("MadeUp", Variant::Enum(7)),
        Some(EditKind::Text("7".to_owned()))
    );
}

#[test]
fn vector3_and_cframe_edit_as_three_labeled_fields() {
    let vector = vector3(1.0, 2.0, 3.0);
    let expected = Some(EditKind::Fields {
        fields: VECTOR3,
        values: vec!["1".to_owned(), "2".to_owned(), "3".to_owned()],
    });

    // Not `Size`: that name is real on `BasePart` and, per the dump's
    // Serialization, not saveable — read-only there now, which this
    // assertion isn't testing (see `a_non_hidden_non_serializable_property_
    // still_shows_but_has_no_edit_affordance` for that behaviour).
    assert_eq!(edit_kind("Extents", Variant::Vector3(vector)), expected);
}

/// A `CFrame` is two captioned lines, not one — its rotation is as editable
/// as its position (see `edit::orientation`).
#[test]
fn cframe_edits_as_a_position_and_an_orientation() {
    let frame = CFrameData {
        position: Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };

    let Some(EditKind::Groups { groups, values }) = edit_kind("CFrame", Variant::CFrame(frame))
    else {
        panic!("a CFrame should edit as captioned groups");
    };

    assert_eq!(
        groups.iter().map(|group| group.caption).collect::<Vec<_>>(),
        ["Position", "Orientation"]
    );
    assert_eq!(values, ["1", "2", "3", "0", "0", "0"]);
}

// An absent optional used to have no editor at all, which left the value
// stuck: nothing in the panel could give it one.
#[test]
fn an_absent_optional_cframe_edits_as_an_unchecked_box_over_a_cframe() {
    let Some(EditKind::Optional { present, inner, .. }) =
        edit_kind("WorldPivotData", Variant::OptionalCFrame(None))
    else {
        panic!("an OptionalCFrame should edit as a present/absent box");
    };

    assert!(!present, "an absent optional reads as unchecked");
    // Seeded even while absent — this is the value the box turns on to.
    assert_eq!(
        *inner,
        EditKind::Groups {
            groups: CFRAME,
            values: ["0", "0", "0", "0", "0", "0"].map(str::to_owned).to_vec(),
        }
    );
}

#[test]
fn a_present_optional_cframe_keeps_the_cframe_editor_under_a_checked_box() {
    let frame = CFrameData {
        position: vector3(1.0, 2.0, 3.0),
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };

    let Some(EditKind::Optional { present, inner, .. }) =
        edit_kind("WorldPivotData", Variant::OptionalCFrame(Some(frame)))
    else {
        panic!("an OptionalCFrame should edit as a present/absent box");
    };

    assert!(present);
    let EditKind::Groups { groups, values } = *inner else {
        panic!("a present optional CFrame edits through the CFrame editor");
    };
    assert_eq!(
        groups.iter().map(|group| group.caption).collect::<Vec<_>>(),
        ["Position", "Orientation"]
    );
    assert_eq!(values, ["1", "2", "3", "0", "0", "0"]);
}

/// `PhysicalProperties` was the last of the three types the roadmap listed
/// as "still read-only text" — and the one that needed a shape, not just a
/// field list, because it is an enum rather than a struct.
#[test]
fn default_physical_properties_edit_as_an_unticked_custom_box() {
    let Some(EditKind::Optional {
        present,
        label,
        inner,
    }) = edit_kind(
        "CustomPhysicalProperties",
        Variant::PhysicalProperties(PhysicalProperties::Default),
    )
    else {
        panic!("PhysicalProperties should edit as a Custom box over five fields");
    };

    assert!(!present, "a Default reads as unticked");
    assert_eq!(label, "Custom");
    // Seeded even while unticked — this is what the box turns on to.
    assert_eq!(
        *inner,
        EditKind::Fields {
            fields: PHYSICAL_PROPERTIES,
            values: ["0.7", "0.3", "0.5", "1", "1"].map(str::to_owned).to_vec(),
        }
    );
}

#[test]
fn custom_physical_properties_edit_as_five_fields_under_a_ticked_box() {
    let Some(EditKind::Optional { present, inner, .. }) = edit_kind(
        "CustomPhysicalProperties",
        Variant::PhysicalProperties(PhysicalProperties::Custom {
            density: 2.5,
            friction: 0.4,
            elasticity: 0.1,
            friction_weight: 3.0,
            elasticity_weight: 0.25,
        }),
    ) else {
        panic!("PhysicalProperties should edit as a Custom box over five fields");
    };

    assert!(present);
    assert_eq!(
        *inner,
        EditKind::Fields {
            fields: PHYSICAL_PROPERTIES,
            values: ["2.5", "0.4", "0.1", "3", "0.25"]
                .map(str::to_owned)
                .to_vec(),
        }
    );
}

/// The five captions are the panel's only clue to which number is which, and
/// Roblox's own constructor order is the one a reader will be comparing to.
#[test]
fn the_physical_properties_fields_are_captioned_in_roblox_order() {
    assert_eq!(
        PHYSICAL_PROPERTIES
            .iter()
            .map(|field| field.label)
            .collect::<Vec<_>>(),
        [
            "Density",
            "Friction",
            "Elasticity",
            "Friction Weight",
            "Elasticity Weight"
        ]
    );
}

#[test]
fn vector2_edits_as_two_labeled_fields() {
    assert_eq!(
        edit_kind("Offset", Variant::Vector2(Vector2Data { x: 1.5, y: -2.0 })),
        Some(EditKind::Fields {
            fields: VECTOR2,
            values: vec!["1.5".to_owned(), "-2".to_owned()],
        })
    );
}

#[test]
fn rect_edits_as_four_labeled_fields() {
    let slice_center = Rect {
        min: Vector2Data { x: 1.0, y: 2.0 },
        max: Vector2Data { x: 3.0, y: 4.0 },
    };
    assert_eq!(
        edit_kind("SliceCenter", Variant::Rect(slice_center)),
        Some(EditKind::Fields {
            fields: RECT,
            values: vec![
                "1".to_owned(),
                "2".to_owned(),
                "3".to_owned(),
                "4".to_owned()
            ],
        })
    );
}

#[test]
fn font_face_edits_as_family_weight_and_style_fields() {
    let face = Font {
        family: "rbxasset://fonts/families/FredokaOne.json".to_owned(),
        weight: 600,
        style: FontStyle::Normal,
        cached_face_id: Some("stale".to_owned()),
    };
    assert_eq!(
        edit_kind("FontFace", Variant::Font(face)),
        Some(EditKind::Fields {
            fields: FONT,
            values: vec![
                "FredokaOne".to_owned(),
                "SemiBold".to_owned(),
                "Normal".to_owned()
            ],
        })
    );
}

#[test]
fn udim2_edits_as_four_labeled_fields() {
    let position = UDim2 {
        x: UDim {
            scale: 0.0,
            offset: 4,
        },
        y: UDim {
            scale: 1.0,
            offset: -4,
        },
    };
    assert_eq!(
        // Not `Position`: that name is real on `BasePart` and tagged
        // `Hidden` in the dump, so the row wouldn't exist at all — this
        // assertion only cares about plain `UDim2` field-splitting.
        edit_kind("GuiPosition", Variant::UDim2(position)),
        Some(EditKind::Fields {
            fields: UDIM2,
            values: vec![
                "0".to_owned(),
                "4".to_owned(),
                "1".to_owned(),
                "-4".to_owned()
            ],
        })
    );
}

#[test]
fn rows_group_by_category_in_alphabetical_order_with_no_empty_groups() {
    let rows = properties(&[
        ("Anchored", Variant::Bool(true)),
        (
            "Color",
            Variant::Color3(Color3Data {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            }),
        ),
        ("CanCollide", Variant::Bool(true)),
    ])
    .rows(part());

    let groups = group_by_category(rows);
    let categories: Vec<&str> = groups
        .iter()
        .map(|(category, _)| category.as_str())
        .collect();
    // `Data` comes from the synthesized `Name` row every instance carries.
    assert_eq!(categories, ["Appearance", "Collision", "Data", "Part"]);
    assert!(groups.iter().all(|(_, rows)| !rows.is_empty()));
}

/// A Workspace holding one instance of `class` carrying `values`, for the
/// rows whose shape depends on which class the property sits on.
fn instance_of(class: &str, values: &[(&str, Variant)]) -> Fixture {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(workspace(), "Workspace", "Workspace"));
    let mut instance = Instance::new(part(), class, "Greeter");
    for (name, value) in values {
        instance
            .properties_mut()
            .insert((*name).to_owned(), value.clone());
    }
    dom.insert(instance);
    dom.set_parent(part(), Some(workspace()));

    Fixture {
        dom,
        properties: Properties::new(ReflectionDatabase::embedded()),
    }
}

fn source_row(class: &str, source: &str) -> PropertyRow {
    instance_of(class, &[("Source", Variant::String(source.to_owned()))])
        .rows(part())
        .into_iter()
        .find(|row| row.name == "Source")
        .expect("the Source row")
}

#[test]
fn a_scripts_source_row_is_read_only_here_because_the_script_editor_owns_it() {
    for class in ["Script", "LocalScript", "ModuleScript"] {
        assert_eq!(
            source_row(class, "print('hi')").edit,
            None,
            "{class}'s Source must not offer a one-line field that would trim the code"
        );
    }
}

#[test]
fn a_scripts_source_row_still_shows_a_summary_of_the_code() {
    // Read-only is not hidden: the row stays, spelled the way any other
    // string is, so the panel still tells you the script has source.
    let row = source_row("Script", "print('hi')");
    assert_eq!(row.value, "\"print('hi')\"");
}

#[test]
fn a_non_script_string_property_is_still_editable_here() {
    // The read-only rule is about `Source` on a script, not about strings.
    let editable = instance_of("StringValue", &[("Value", Variant::String("x".into()))])
        .rows(part())
        .into_iter()
        .find(|row| row.name == "Value")
        .expect("the Value row");
    assert_eq!(editable.edit, Some(EditKind::Text("x".to_owned())));
}

#[test]
fn hidden_properties_never_appear_as_rows_at_all() {
    // Position/Orientation are tagged Hidden in the real dump: Studio only
    // exposes them through the dedicated Position/Orientation UI, never as
    // a raw property row — not even a read-only one.
    let rows = instance_of(
        "Part",
        &[
            ("Position", Variant::Vector3(vector3(1.0, 2.0, 3.0))),
            ("Orientation", Variant::Vector3(vector3(0.0, 0.0, 0.0))),
        ],
    )
    .rows(part());

    assert!(!rows.iter().any(|row| row.name == "Position"));
    assert!(!rows.iter().any(|row| row.name == "Orientation"));
}

#[test]
fn a_non_hidden_non_serializable_property_still_shows_but_has_no_edit_affordance() {
    // BasePart.Size is not Hidden but Serialization.CanSave is false in the
    // real dump (Studio derives it rather than storing it directly): the
    // row must stay, just without an edit widget.
    let row = instance_of(
        "Part",
        &[("Size", Variant::Vector3(vector3(4.0, 1.0, 2.0)))],
    )
    .rows(part())
    .into_iter()
    .find(|row| row.name == "Size")
    .expect("the Size row");

    assert_eq!(row.edit, None);
}

#[test]
fn cframe_stays_visible_and_editable_after_hidden_filtering() {
    // The property this codebase actually uses in place of the not-yet-built
    // Position/Orientation UI: it must not get caught by the Hidden filter
    // just because Position/Orientation (which it stands in for) did.
    let frame = CFrameData {
        position: vector3(0.0, 5.0, 0.0),
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };
    let row = instance_of("Part", &[("CFrame", Variant::CFrame(frame))])
        .rows(part())
        .into_iter()
        .find(|row| row.name == "CFrame")
        .expect("the CFrame row");

    let Some(EditKind::Groups { values, .. }) = row.edit else {
        panic!("a CFrame should still edit as captioned groups after filtering");
    };
    assert_eq!(values, ["0", "5", "0", "0", "0", "0"]);
}

#[test]
fn an_ordinary_property_is_unaffected_by_hidden_or_read_only_filtering() {
    let row = instance_of("Part", &[("Name", Variant::String("Baseplate".into()))])
        .rows(part())
        .into_iter()
        .find(|row| row.name == "Name")
        .expect("the Name row");

    assert_eq!(row.edit, Some(EditKind::Text("Baseplate".to_owned())));
}

#[test]
fn a_source_property_on_a_class_that_is_not_a_script_stays_editable() {
    // Only `LuaSourceContainer` hands its Source to the script editor; a
    // property that merely shares the name elsewhere is untouched.
    assert_eq!(
        source_row("Part", "not code").edit,
        Some(EditKind::Text("not code".to_owned()))
    );
}
