//! Encoder for Content. Mirrors `value::content`; the legacy `ContentId` tag is
//! never written since `Variant::Content` carries no bit remembering which of
//! the two produced it, and the reader decodes both to the same in-memory type.

use rbx_dom::Content;

use crate::serializer::referent;
use crate::serializer::writer::Writer;

pub(crate) fn content(writer: &mut Writer, name: &str, value: &Content) {
    writer.open("Content", &[("name", name)]);
    match value {
        Content::None => writer.leaf("null", &[], ""),
        Content::Uri(uri) => writer.leaf("uri", &[], uri),
        Content::Object(target) => writer.leaf("Ref", &[], &referent(*target)),
    }
    writer.close("Content");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Ref;

    #[test]
    fn object_writes_a_ref_child() {
        let mut writer = Writer::new();
        content(&mut writer, "X", &Content::Object(Ref::new(7)));
        assert!(writer.into_string().contains("<Ref>RBX7</Ref>"));
    }

    #[test]
    fn none_writes_a_null_child() {
        let mut writer = Writer::new();
        content(&mut writer, "X", &Content::None);
        assert!(writer.into_string().contains("<null></null>"));
    }
}
