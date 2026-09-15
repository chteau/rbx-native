//! Decoder for Font: `Family`/`CachedFaceId` are Content-shaped (`url`/`uri`
//! child); `Weight` is a plain int; `Style` is a plain enum-name string.

use rbx_dom::{Font, FontStyle, Variant};

use crate::xml_tree::Node;

pub(crate) fn font(node: &Node) -> Variant {
    let family = node.child("Family").map(content_text).unwrap_or_default();
    let weight = node
        .child("Weight")
        .and_then(|c| c.text_trim().parse().ok())
        .unwrap_or(0);
    let style = node
        .child("Style")
        .map(|c| style_from_name(c.text_trim()))
        .unwrap_or(FontStyle::Normal);
    let cached_face_id = node.child("CachedFaceId").and_then(|c| {
        let text = content_text(c);
        (!text.is_empty()).then_some(text)
    });

    Variant::Font(Font {
        family,
        weight,
        style,
        cached_face_id,
    })
}

// `Family`/`CachedFaceId` wrap a nested Content-ish element (`url` or `uri`, or
// `null` for absence); only the URI string is kept, since `Font` has no field for
// a referent-typed source.
fn content_text(node: &Node) -> String {
    node.child("url")
        .or_else(|| node.child("uri"))
        .map(|c| c.text_trim().to_owned())
        .unwrap_or_default()
}

// `Other` is not itself reachable from documented XML (xml.md only lists Normal
// and Italic as `Style` text), but keeps this decoder as future-proof as
// `FontStyle::from(u8)` is for the binary format, for a value Roblox has since added.
fn style_from_name(name: &str) -> FontStyle {
    match name {
        "Italic" => FontStyle::Italic,
        "Normal" => FontStyle::Normal,
        _ => FontStyle::Other(0xFF),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_reads_the_spec_example() {
        let node = Node {
            tag: "Font".into(),
            children: vec![
                Node {
                    tag: "Family".into(),
                    children: vec![Node {
                        tag: "url".into(),
                        text: "rbxasset://fonts/families/Arial.json".into(),
                        ..Node::default()
                    }],
                    ..Node::default()
                },
                Node {
                    tag: "Weight".into(),
                    text: "700".into(),
                    ..Node::default()
                },
                Node {
                    tag: "Style".into(),
                    text: "Italic".into(),
                    ..Node::default()
                },
            ],
            ..Node::default()
        };

        assert_eq!(
            font(&node),
            Variant::Font(Font {
                family: "rbxasset://fonts/families/Arial.json".to_owned(),
                weight: 700,
                style: FontStyle::Italic,
                cached_face_id: None,
            })
        );
    }
}
