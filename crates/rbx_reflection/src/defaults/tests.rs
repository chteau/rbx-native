use super::*;

const API_DUMP_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/API-Dump.json"
));
const DEFAULTS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/reflection-defaults.json"
));

fn database() -> ReflectionDatabase {
    ReflectionDatabase::from_json_str(API_DUMP_JSON)
        .and_then(|database| database.with_defaults(DEFAULTS_JSON))
        .expect("bundled assets must parse")
}

#[test]
fn a_part_defaults_to_what_studio_inserts() {
    let db = database();

    assert_eq!(
        db.default_value("Part", "Anchored"),
        Some(&Variant::Bool(false))
    );
    assert_eq!(
        db.default_value("Part", "CanCollide"),
        Some(&Variant::Bool(true))
    );
    assert_eq!(
        db.default_value("Part", "Color"),
        Some(&Variant::Color3uint8 {
            r: 163,
            g: 162,
            b: 165
        })
    );
    assert_eq!(
        db.default_value("Part", "Size"),
        Some(&Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 1.2,
            z: 2.0
        }))
    );
    // `Enum.PartType.Block`, and `Enum.Material.Plastic`.
    assert_eq!(db.default_value("Part", "Shape"), Some(&Variant::Enum(1)));
    assert_eq!(
        db.default_value("Part", "Material"),
        Some(&Variant::Enum(256))
    );
}

#[test]
fn an_abstract_class_has_no_defaults_of_its_own() {
    let db = database();

    assert_eq!(db.default_value("BasePart", "Anchored"), None);
    assert_eq!(db.default_value("NotAClass", "Anchored"), None);
}

#[test]
fn a_cframe_default_keeps_its_rows_in_order() {
    let db = database();

    // A new Camera sits at (0, 20, 20) looking down at the origin: its look
    // vector, the negated third column, points down and towards -Z.
    let Some(Variant::CFrame(frame)) = db.default_value("Camera", "CFrame") else {
        panic!("a Camera has a default CFrame");
    };
    assert_eq!(frame.position.y, 20.0);
    assert!(frame.rotation[5] > 0.7 && frame.rotation[8] > 0.7);
    assert!(frame.rotation[7] < -0.7);
}

#[test]
fn a_font_weight_is_turned_from_its_name_into_its_number() {
    let db = database();

    let Some(Variant::Font(font)) = db.default_value("TextLabel", "FontFace") else {
        panic!("a TextLabel has a default FontFace");
    };
    assert_eq!(font.weight, 400);
    assert_eq!(font.style, FontStyle::Normal);
}

#[test]
fn a_saved_spelling_resolves_to_the_property_it_holds() {
    let db = database();

    assert_eq!(db.canonical_name("Part", "size"), "Size");
    assert_eq!(db.canonical_name("Part", "Color3uint8"), "Color");
    assert_eq!(db.canonical_name("Part", "shape"), "Shape");
    assert_eq!(db.canonical_name("Part", "Anchored"), "Anchored");
    // Declared on `Part`, not `BasePart`: a wedge has no `shape`.
    assert_eq!(db.canonical_name("WedgePart", "shape"), "shape");
}

#[test]
fn stored_names_put_the_saved_spelling_first() {
    let db = database();

    assert_eq!(db.stored_names("Part", "Size"), ["size", "Size"]);
    assert_eq!(db.stored_names("Part", "size"), ["size", "Size"]);
    assert_eq!(db.stored_names("Part", "Color"), ["Color3uint8", "Color"]);
    assert_eq!(db.stored_names("Part", "Anchored"), ["Anchored"]);
    // Two legacy spellings, and one saved one.
    assert_eq!(
        db.stored_names("Fire", "Size"),
        ["size_xml", "Size", "size"]
    );
    assert_eq!(db.stored_names("Part", "NotAProperty"), ["NotAProperty"]);
}

#[test]
fn a_database_without_defaults_still_answers_by_name() {
    let db = ReflectionDatabase::from_json_str(API_DUMP_JSON).unwrap();

    assert_eq!(db.default_value("Part", "Anchored"), None);
    assert_eq!(db.canonical_name("Part", "size"), "size");
    assert_eq!(db.stored_names("Part", "Size"), ["Size"]);
}

#[test]
fn a_value_the_conversion_cannot_read_is_dropped_alone() {
    let json = r#"{"Classes": {"Thing": {"Defaults": {
        "Kept": {"Bool": true},
        "Unknown": {"MaterialColors": {}},
        "Malformed": {"Vector3": [1, 2]}
    }}}}"#;
    let defaults = Defaults::parse(json, |_| None).unwrap();

    let values = &defaults.classes["Thing"].values;
    assert_eq!(values.get("Kept"), Some(&Variant::Bool(true)));
    assert_eq!(values.len(), 1);
}
