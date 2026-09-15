//! A minimal generic XML tree, parsed once up front.
//!
//! Roblox's XML format nests `Item`s inside `Item`s and reads properties by child
//! tag name, both easier to walk over a small materialized tree than by driving
//! `quick_xml`'s event stream by hand through every recursive call site.

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::XmlError;

/// One element of the document, with its attributes, text content, and children.
///
/// `text` collects every `Text`/`CData`/resolved character-reference run directly
/// inside this element, in document order; mixed content (text interleaved with
/// child elements) is not distinguished from purely textual content, since no
/// Roblox XML type needs that distinction.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Node {
    pub(crate) tag: String,
    // `pub(crate)` (not private) so decoders in `value`'s tests can build a
    // synthetic `Node` with struct-literal syntax without a builder function.
    pub(crate) attrs: Vec<(String, String)>,
    pub(crate) children: Vec<Node>,
    pub(crate) text: String,
}

impl Node {
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn child(&self, tag: &str) -> Option<&Node> {
        self.children.iter().find(|node| node.tag == tag)
    }

    pub(crate) fn children_named<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |node| node.tag == tag)
    }

    pub(crate) fn text_trim(&self) -> &str {
        self.text.trim()
    }
}

/// Parses `input` into a synthetic root `Node` (tag `"#document"`) whose children
/// are the document's top-level elements (normally just one `roblox` element).
pub(crate) fn parse(input: &str) -> Result<Node, XmlError> {
    let mut reader = Reader::from_str(input);
    let mut stack = vec![Node {
        tag: "#document".to_owned(),
        ..Node::default()
    }];

    loop {
        match reader.read_event().map_err(parse_err)? {
            Event::Eof => break,
            Event::Start(start) => {
                let tag = local_name(start.name().as_ref());
                let attrs = read_attrs(&start)?;
                stack.push(Node {
                    tag,
                    attrs,
                    ..Node::default()
                });
            }
            Event::Empty(start) => {
                let tag = local_name(start.name().as_ref());
                let attrs = read_attrs(&start)?;
                attach(
                    &mut stack,
                    Node {
                        tag,
                        attrs,
                        ..Node::default()
                    },
                );
            }
            Event::End(_) => {
                // A well-formed document (checked by the reader's default
                // `check_end_names`) never pops the synthetic document root here.
                if let Some(node) = stack.pop() {
                    attach(&mut stack, node);
                }
            }
            Event::Text(text) => push_text(&mut stack, &text.decode().map_err(parse_err)?),
            // CData content is literal by definition: no entity resolution applies.
            Event::CData(cdata) => push_text(&mut stack, &cdata.decode().map_err(parse_err)?),
            Event::GeneralRef(reference) => {
                push_text(&mut stack, &resolve_general_ref(&reference)?)
            }
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
    }

    Ok(stack.into_iter().next().unwrap_or_default())
}

fn attach(stack: &mut [Node], node: Node) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    }
}

fn push_text(stack: &mut [Node], text: &str) {
    if let Some(top) = stack.last_mut() {
        top.text.push_str(text);
    }
}

fn read_attrs(
    start: &quick_xml::events::BytesStart<'_>,
) -> Result<Vec<(String, String)>, XmlError> {
    start
        .attributes()
        .map(|attr| {
            let attr = attr.map_err(parse_err)?;
            let key = local_name(attr.key.as_ref());
            let value = attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(parse_err)?
                .into_owned();
            Ok((key, value))
        })
        .collect()
}

// Numeric character references (`&#38;`) resolve directly; named ones are limited
// to XML's five predefined entities since Roblox's format declares no DTD entities.
// An entity outside that set is preserved literally rather than dropped, so a file
// using a (non-conformant) custom entity still round-trips its text visibly.
fn resolve_general_ref(reference: &quick_xml::events::BytesRef<'_>) -> Result<String, XmlError> {
    if let Some(c) = reference.resolve_char_ref().map_err(parse_err)? {
        return Ok(c.to_string());
    }
    let name = reference.decode().map_err(parse_err)?;
    match resolve_predefined_entity(&name) {
        Some(resolved) => Ok(resolved.to_owned()),
        None => Ok(format!("&{name};")),
    }
}

// Namespaces are never used in Roblox XML; stripping any prefix keeps callers from
// having to special-case `xmlns`-qualified names that never actually occur.
fn local_name(qname: &[u8]) -> String {
    let name = match qname.iter().position(|&b| b == b':') {
        Some(colon) => &qname[colon + 1..],
        None => qname,
    };
    String::from_utf8_lossy(name).into_owned()
}

fn parse_err(err: impl std::fmt::Display) -> XmlError {
    XmlError::Parse(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_items_and_attrs() {
        let doc = parse(
            r#"<roblox version="4"><Item class="Part" referent="A"><Properties><string name="Name">Hi</string></Properties></Item></roblox>"#,
        )
        .unwrap();

        let roblox = doc.child("roblox").unwrap();
        let item = roblox.child("Item").unwrap();
        assert_eq!(item.attr("class"), Some("Part"));
        assert_eq!(item.attr("referent"), Some("A"));

        let props = item.child("Properties").unwrap();
        let name_prop = props.child("string").unwrap();
        assert_eq!(name_prop.attr("name"), Some("Name"));
        assert_eq!(name_prop.text_trim(), "Hi");
    }

    #[test]
    fn resolves_entities_and_char_refs() {
        let doc = parse(r#"<roblox><string name="X">A &amp; B &#65;</string></roblox>"#).unwrap();
        let node = doc.child("roblox").unwrap().child("string").unwrap();
        assert_eq!(node.text_trim(), "A & B A");
    }

    #[test]
    fn preserves_cdata_verbatim() {
        let doc = parse(
            r#"<roblox><ProtectedString name="X"><![CDATA[a < b & c]]></ProtectedString></roblox>"#,
        )
        .unwrap();
        let node = doc
            .child("roblox")
            .unwrap()
            .child("ProtectedString")
            .unwrap();
        assert_eq!(node.text_trim(), "a < b & c");
    }
}
