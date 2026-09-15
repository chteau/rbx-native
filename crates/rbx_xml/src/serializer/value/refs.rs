//! Encoder for the `Ref` property type. Mirrors `value::refs`.
//!
//! A null `Ref` is never encoded here: like the reader, this crate represents
//! "no value" by the property being absent from the map entirely (see
//! `serializer`'s property loop), not by a `Ref` variant holding a sentinel.

use rbx_dom::Ref;

use crate::serializer::referent;
use crate::serializer::writer::Writer;

pub(crate) fn reference(writer: &mut Writer, name: &str, target: Ref) {
    writer.leaf("Ref", &[("name", name)], &referent(target));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_rbx_prefixed_referent() {
        let mut writer = Writer::new();
        reference(&mut writer, "Target", Ref::new(3));
        assert_eq!(writer.into_string(), "<Ref name=\"Target\">RBX3</Ref>\n");
    }
}
