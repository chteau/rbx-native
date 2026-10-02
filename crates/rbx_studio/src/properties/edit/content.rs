//! A `Content` row (`Decal.Texture`, `MeshPart.MeshId`) as an asset URI
//! field. Only the `None` and `Uri` sources are typed: an `Object` source
//! names an instance in the place, which needs an instance picker rather
//! than text, so such a row stays read-only.

use rbx_dom::{Content, Variant};

/// The URI itself, or an empty field for `None`. `None` for an `Object`
/// source keeps the row read-only, the same as any type [`super::parse`]
/// cannot read back.
pub(super) fn content_text(content: &Content) -> Option<String> {
    match content {
        Content::None => Some(String::new()),
        Content::Uri(uri) => Some(uri.clone()),
        Content::Object(_) => None,
    }
}

/// Follows Roblox's own two constructors (creator-docs, `Content.yaml`):
/// `Content.fromUri` returns `Content.none` for an empty string, and
/// `Content.fromAssetId(id)` is `fromUri("rbxassetid://" .. id)` with `0`
/// also meaning none. So an empty field clears the value, a bare number is
/// an asset id, and anything else is kept as the URI it was typed as —
/// asset URIs have several schemes (`rbxasset://`, `rbxthumb://`, plain
/// `https://`), and refusing the ones not listed here would refuse real
/// values. A number that is no asset id (negative, fractional, past `u64`)
/// is refused rather than kept as a URI nothing can load.
///
/// A file can hold an empty URI rather than none (rbx_xml reads an empty
/// `<url>` that way); an empty field leaves it as it is, so committing an
/// untouched row is no edit.
pub(super) fn parse_content(current: &Content, text: &str) -> Result<Variant, String> {
    let text = text.trim();
    let content = match text.parse::<u64>() {
        Ok(0) => Content::None,
        Ok(id) => Content::Uri(format!("rbxassetid://{id}")),
        Err(_) if text.is_empty() => match current {
            Content::Uri(uri) if uri.is_empty() => current.clone(),
            _ => Content::None,
        },
        Err(_) if text.parse::<f64>().is_ok() => {
            return Err(format!("{text:?} is not an asset id"))
        }
        Err(_) => Content::Uri(text.to_owned()),
    };
    Ok(Variant::Content(content))
}

#[cfg(test)]
mod tests {
    use rbx_dom::Ref;

    use super::*;

    fn uri(text: &str) -> Variant {
        Variant::Content(Content::Uri(text.to_owned()))
    }

    fn parse(text: &str) -> Result<Variant, String> {
        parse_content(&Content::None, text)
    }

    #[test]
    fn none_and_uri_sources_are_typed_text() {
        assert_eq!(content_text(&Content::None), Some(String::new()));
        assert_eq!(
            content_text(&Content::Uri("rbxassetid://12".into())),
            Some("rbxassetid://12".to_owned())
        );
    }

    #[test]
    fn an_object_source_stays_read_only() {
        assert_eq!(content_text(&Content::Object(Ref::new(3))), None);
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
    fn every_typed_value_round_trips() {
        for content in [
            Content::None,
            Content::Uri(String::new()),
            Content::Uri("rbxassetid://9".into()),
        ] {
            let text = content_text(&content).unwrap();
            assert_eq!(
                parse_content(&content, &text),
                Ok(Variant::Content(content))
            );
        }
    }
}
