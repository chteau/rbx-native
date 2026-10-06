use rbx_dom::Ref;

use super::*;

mod object;

fn uri(text: &str) -> Variant {
    Variant::Content(Content::Uri(text.to_owned()))
}

fn parse(text: &str) -> Result<Variant, String> {
    parse_content(&Content::None, text)
}

#[test]
fn none_and_uri_sources_are_typed_text() {
    assert_eq!(content_text(&Content::None), "");
    assert_eq!(
        content_text(&Content::Uri("rbxassetid://12".into())),
        "rbxassetid://12"
    );
}

#[test]
fn a_uri_is_kept_as_typed_after_trimming() {
    assert_eq!(parse("  rbxassetid://7 "), Ok(uri("rbxassetid://7")));
    assert_eq!(
        parse("rbxasset://textures/face.png"),
        Ok(uri("rbxasset://textures/face.png"))
    );
    assert_eq!(
        parse("rbxthumb://type=Asset&id=1&w=150&h=150"),
        Ok(uri("rbxthumb://type=Asset&id=1&w=150&h=150"))
    );
}

#[test]
fn a_bare_number_is_an_asset_id() {
    assert_eq!(parse("1818"), Ok(uri("rbxassetid://1818")));
}

#[test]
fn an_empty_field_or_asset_zero_clears_the_value() {
    assert_eq!(parse(""), Ok(Variant::Content(Content::None)));
    assert_eq!(parse("   "), Ok(Variant::Content(Content::None)));
    assert_eq!(parse("0"), Ok(Variant::Content(Content::None)));
}

#[test]
fn a_number_that_is_no_asset_id_is_refused() {
    for text in ["-5", "1.5", "99999999999999999999"] {
        assert!(parse(text).is_err(), "{text} was accepted");
    }
}

#[test]
fn committing_an_asset_id_to_a_decal_writes_a_uri() {
    use rbx_dom::{Instance, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    let decal = Ref::new(1);
    let mut dom = WeakDom::new();
    let mut instance = Instance::new(decal, "Decal", "Decal");
    instance
        .properties_mut()
        .insert("Texture".to_owned(), Variant::Content(Content::None));
    dom.insert(instance);

    let previous = crate::properties::edit::commit(
        &mut dom,
        &ReflectionDatabase::embedded(),
        decal,
        "Texture",
        "1818",
    )
    .expect("an asset id is a valid Content value");

    assert_eq!(previous, Some(Variant::Content(Content::None)));
    assert_eq!(
        dom.get(decal).unwrap().properties().get("Texture"),
        Some(&uri("rbxassetid://1818"))
    );
}

#[test]
fn a_string_stored_texture_reads_an_asset_id_and_stays_a_string() {
    use rbx_dom::{Instance, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    let decal = Ref::new(1);
    let mut dom = WeakDom::new();
    let mut instance = Instance::new(decal, "Decal", "Decal");
    instance.properties_mut().insert(
        "Texture".to_owned(),
        Variant::String("rbxassetid://1".into()),
    );
    dom.insert(instance);
    let db = ReflectionDatabase::embedded();
    let commit = |dom: &mut WeakDom, text: &str| {
        crate::properties::edit::commit(dom, &db, decal, "Texture", text).unwrap();
        dom.get(decal).unwrap().properties().get("Texture").cloned()
    };

    let string = |s: &str| Some(Variant::String(s.to_owned()));
    assert_eq!(commit(&mut dom, "12345"), string("rbxassetid://12345"));
    assert_eq!(
        commit(&mut dom, "rbxasset://a.png"),
        string("rbxasset://a.png")
    );
    assert_eq!(commit(&mut dom, "0"), string(""));
    assert!(crate::properties::edit::commit(&mut dom, &db, decal, "Texture", "1.5").is_err());
}

#[test]
fn every_typed_value_round_trips() {
    for content in [
        Content::None,
        Content::Uri(String::new()),
        Content::Uri("rbxassetid://9".into()),
        Content::Object(Ref::new(3)),
    ] {
        let text = content_text(&content);
        assert_eq!(
            parse_content(&content, &text),
            Ok(Variant::Content(content))
        );
    }
}
