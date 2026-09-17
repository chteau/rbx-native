//! Decoders for Content and the legacy ContentId, both represented in-memory as
//! `rbx_dom::Content`.

use rbx_dom::{Content, Variant};

use super::Ctx;
use crate::xml_tree::Node;

pub(crate) fn content(node: &Node, ctx: &Ctx<'_>) -> Variant {
    let value = if node.child("null").is_some() {
        Content::None
    } else if let Some(uri) = node.child("uri").or_else(|| node.child("url")) {
        // `url` is the child the older `ContentId` spelling used, and what a
        // catalogue model asset (a `Decal` wrapper) still carries under a
        // `Content` tag: same string, same meaning.
        Content::Uri(uri.text_trim().to_owned())
    } else if let Some(reference) = node.child("Ref") {
        // An unresolved referent is the same as no source at all: the property
        // still decodes, it just carries nothing.
        ctx.referents
            .get(reference.text_trim())
            .map_or(Content::None, |&r| Content::Object(r))
    } else {
        Content::None
    };
    Variant::Content(value)
}

// ContentId predates Content (renamed at Roblox release 645) but decodes to the
// same in-memory type; the legacy `binary`/`hash` children are always empty per
// xml.md, so anything other than `url` is treated as absence.
pub(crate) fn content_id(node: &Node) -> Variant {
    let value = match node.child("url") {
        Some(url) => Content::Uri(url.text_trim().to_owned()),
        None => Content::None,
    };
    Variant::Content(value)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rbx_dom::Ref;

    use super::*;

    fn ctx<'a>(
        referents: &'a HashMap<String, Ref>,
        shared: &'a HashMap<String, Vec<u8>>,
    ) -> Ctx<'a> {
        Ctx { referents, shared }
    }

    #[test]
    fn content_null_child_is_none() {
        let node = Node {
            tag: "Content".into(),
            children: vec![Node {
                tag: "null".into(),
                ..Node::default()
            }],
            ..Node::default()
        };
        let (referents, shared) = (HashMap::new(), HashMap::new());
        assert_eq!(
            content(&node, &ctx(&referents, &shared)),
            Variant::Content(Content::None)
        );
    }

    #[test]
    fn content_ref_child_resolves_against_referents() {
        let target = Ref::new(7);
        let mut referents = HashMap::new();
        referents.insert("RBXTarget".to_owned(), target);
        let shared = HashMap::new();

        let node = Node {
            tag: "Content".into(),
            children: vec![Node {
                tag: "Ref".into(),
                text: "RBXTarget".into(),
                ..Node::default()
            }],
            ..Node::default()
        };
        assert_eq!(
            content(&node, &ctx(&referents, &shared)),
            Variant::Content(Content::Object(target))
        );
    }

    #[test]
    fn content_url_child_is_uri_like_the_legacy_spelling() {
        let node = Node {
            tag: "Content".into(),
            children: vec![Node {
                tag: "url".into(),
                text: "http://www.roblox.com/asset/?id=6891610105".into(),
                ..Node::default()
            }],
            ..Node::default()
        };
        let (referents, shared) = (HashMap::new(), HashMap::new());
        assert_eq!(
            content(&node, &ctx(&referents, &shared)),
            Variant::Content(Content::Uri(
                "http://www.roblox.com/asset/?id=6891610105".to_owned()
            ))
        );
    }

    #[test]
    fn content_id_url_child_is_uri() {
        let node = Node {
            tag: "ContentId".into(),
            children: vec![Node {
                tag: "url".into(),
                text: "rbxasset://textures/face.png".into(),
                ..Node::default()
            }],
            ..Node::default()
        };
        assert_eq!(
            content_id(&node),
            Variant::Content(Content::Uri("rbxasset://textures/face.png".to_owned()))
        );
    }
}
