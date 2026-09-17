use super::*;

/// The shape of the fixture every test below reads: two sheets — a token
/// sheet and a design sheet that derives from it — and the design sheet's two
/// rules, one of them with a property already set.
fn styled() -> (WeakDom, Ref, Ref, Ref, Ref) {
    let mut dom = WeakDom::new();
    let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
    let tokens = dom.new_instance(SHEET_CLASS, "Tokens", Some(storage));
    let design = dom.new_instance(SHEET_CLASS, "Design", Some(storage));

    let derive = dom.new_instance(DERIVE_CLASS, "StyleDerive", Some(design));
    let _ = dom.set_property(derive, PRIORITY_PROPERTY, Variant::Int32(3));
    let _ = dom.set_property(derive, SHEET_PROPERTY, Variant::Ref(tokens));

    let card = add_rule(&mut dom, design);
    let _ = dom.set_property(
        card,
        SELECTOR_PROPERTY,
        Variant::String("Frame.Card".to_owned()),
    );
    let _ = dom.set_property(card, PRIORITY_PROPERTY, Variant::Int32(10));
    set_rule_property(&mut dom, &database(), card, "BorderSizePixel", "4").expect("a new property");

    // Nested inside `card`, the way a merged selector is authored.
    let hover = add_rule(&mut dom, card);
    let _ = dom.set_property(
        hover,
        SELECTOR_PROPERTY,
        Variant::String(":Hover".to_owned()),
    );

    (dom, design, derive, card, hover)
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn every_sheet_derive_rule_and_property_becomes_a_row() {
    let (dom, design, derive, card, hover) = styled();

    let rows = rows(&dom);

    assert_eq!(
        rows,
        vec![
            StyleRow::Sheet {
                referent: explorer::find_by_name(&dom, "Tokens").expect("the token sheet"),
                name: "Tokens".to_owned(),
                parent: "ReplicatedStorage".to_owned(),
            },
            StyleRow::Sheet {
                referent: design,
                name: "Design".to_owned(),
                parent: "ReplicatedStorage".to_owned(),
            },
            StyleRow::Derive {
                referent: derive,
                sheet: "Tokens".to_owned(),
                priority: 3,
            },
            StyleRow::Rule {
                referent: card,
                depth: 0,
                selector: "Frame.Card".to_owned(),
                priority: 10,
            },
            StyleRow::Property {
                rule: card,
                depth: 0,
                name: "BorderSizePixel".to_owned(),
                value: Some("4".to_owned()),
            },
            StyleRow::Rule {
                referent: hover,
                depth: 1,
                selector: ":Hover".to_owned(),
                priority: 0,
            },
        ]
    );
}

/// A property row stands for the rule that holds it, so clicking one selects
/// the rule and the Properties panel shows it.
#[test]
fn a_property_row_selects_its_rule() {
    let (dom, _, _, card, _) = styled();

    let property = rows(&dom)
        .into_iter()
        .find(|row| matches!(row, StyleRow::Property { .. }))
        .expect("the card rule's property");

    assert_eq!(property.referent(), card);
}

/// Adding, changing and dropping a property all go through the blob, so this
/// is the round trip that matters: what is written reads back decoded.
#[test]
fn a_property_written_to_a_rule_reads_back_out_of_the_blob() {
    let (mut dom, _, _, card, _) = styled();
    let database = database();

    set_rule_property(&mut dom, &database, card, "BackgroundColor3", "255, 0, 128")
        .expect("a Color3 the dump knows on Frame");
    set_rule_property(&mut dom, &database, card, "BorderSizePixel", "7").expect("an existing int");

    let values = decode(dom.get(card).expect("the rule"));
    assert_eq!(values.get("BorderSizePixel"), Some(&Variant::Int32(7)));
    assert!(matches!(
        values.get("BackgroundColor3"),
        Some(Variant::Color3(color)) if color.r == 1.0 && color.g == 0.0
    ));

    remove_rule_property(&mut dom, &database, card, "BorderSizePixel").expect("it is there");
    let values = decode(dom.get(card).expect("the rule"));
    assert!(!values.contains_key("BorderSizePixel"));
    assert!(values.contains_key("BackgroundColor3"));
}

/// A `$Token` reference is an ordinary string value, so it survives an edit
/// to a neighbouring property untouched — the cascade resolves it later.
#[test]
fn a_token_reference_survives_a_neighbouring_edit() {
    let (mut dom, _, _, card, _) = styled();
    let database = database();

    set_rule_property(&mut dom, &database, card, "Size", "$CardSize").expect("a string value");
    set_rule_property(&mut dom, &database, card, "BorderSizePixel", "1").expect("an existing int");

    assert_eq!(
        decode(dom.get(card).expect("the rule")).get("Size"),
        Some(&Variant::String("$CardSize".to_owned()))
    );

    // Typing a literal over a token reads the type off the dump again,
    // rather than keeping the string the token was stored as.
    set_rule_property(&mut dom, &database, card, "Size", "0, 200, 0, 200").expect("a UDim2");
    assert!(matches!(
        decode(dom.get(card).expect("the rule")).get("Size"),
        Some(Variant::UDim2(size)) if size.x.offset == 200
    ));
}

/// The type of a property a rule does not yet hold comes from the class the
/// selector names; a tag rule, which names none, falls back to the GUI
/// classes in `FALLBACK_CLASSES`.
#[test]
fn a_property_type_is_found_with_or_without_a_class_in_the_selector() {
    let database = database();

    assert_eq!(class_of(&database, "Frame.Card", "Size"), Some("Frame"));
    assert_eq!(
        class_of(&database, "TextButton:Hover", "TextSize"),
        Some("TextButton")
    );
    // A tag rule: no class in the selector, so the fallback list answers.
    assert_eq!(
        class_of(&database, ".ButtonPrimary", "TextSize"),
        Some("TextBox")
    );
    assert_eq!(class_of(&database, ".ButtonPrimary", "NotAProperty"), None);
}

#[test]
fn only_a_selector_that_opens_with_a_class_names_one() {
    assert_eq!(selector_class("Frame"), Some("Frame"));
    assert_eq!(selector_class(" Frame > TextLabel"), Some("Frame"));
    assert_eq!(selector_class("TextButton:Hover"), Some("TextButton"));
    assert_eq!(selector_class(".Card"), None);
    assert_eq!(selector_class("#MenuFrame"), None);
    assert_eq!(selector_class(":Hover"), None);
    assert_eq!(selector_class(""), None);
}

/// A property no GUI class has cannot be typed, so it is refused rather than
/// guessed at — and the rule's blob is left exactly as it was.
#[test]
fn an_unknown_property_name_is_refused_and_changes_nothing() {
    let (mut dom, _, _, card, _) = styled();
    let before = decode(dom.get(card).expect("the rule"));

    let error = set_rule_property(&mut dom, &database(), card, "Wibble", "1")
        .expect_err("no class has a Wibble");

    assert!(error.contains("Wibble"), "{error}");
    assert_eq!(decode(dom.get(card).expect("the rule")), before);
}

/// A new sheet lands where Studio's own Create Design puts one, and a new
/// rule under it is immediately editable — every property the panel writes
/// through is already present (see `add_rule`).
#[test]
fn a_new_sheet_and_rule_arrive_ready_to_edit() {
    let mut dom = WeakDom::new();
    let sheet = add_sheet(&mut dom);
    let rule = add_rule(&mut dom, sheet);

    assert_eq!(
        dom.parent(sheet)
            .and_then(|parent| dom.get(parent))
            .map(|i| i.class()),
        Some(SHEET_SERVICE)
    );
    let properties = dom.get(rule).expect("the rule").properties();
    assert!(properties.contains_key(SELECTOR_PROPERTY));
    assert!(properties.contains_key(PRIORITY_PROPERTY));
    assert!(properties.contains_key(PROPERTIES_PROPERTY));
}

/// A `StyleLink` only styles the tree under a `LayerCollector`, so the panel
/// refuses to put one anywhere else instead of silently creating a link that
/// does nothing.
#[test]
fn a_style_link_goes_under_a_screen_gui_and_nowhere_else() {
    let (mut dom, design, _, _, _) = styled();
    let database = database();
    let gui = dom.new_instance("ScreenGui", "HUD", None);
    let frame = dom.new_instance("Frame", "Frame", Some(gui));

    let link = add_link(&mut dom, &database, gui, design).expect("a ScreenGui takes a link");
    assert_eq!(dom.get(link).expect("the link").class(), LINK_CLASS);
    assert_eq!(
        dom.get(link)
            .expect("the link")
            .properties()
            .get(SHEET_PROPERTY),
        Some(&Variant::Ref(design))
    );

    assert!(add_link(&mut dom, &database, frame, design).is_err());
}
