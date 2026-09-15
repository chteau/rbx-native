//! Content encoding, the encode counterpart of `chunks::prop::content`.

use rbx_dom::{Content, Variant};

use crate::codec::{encode_referents, interleave_u32};
use crate::serialize::prop::map_dense;
use crate::serialize::writer::Writer;
use crate::serialize::SerializeError;

const SOURCE_NONE: u32 = 0;
const SOURCE_URI: u32 = 1;
const SOURCE_OBJECT: u32 = 2;

/// Writes a column of source tags followed by the URI pool, then the in-file referent
/// pool, then an always-empty external-referent pool: this crate never produces content
/// that points outside its own file.
pub(super) fn contents(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let contents = map_dense(class, name, values, |v| match v {
        Variant::Content(c) => Some(c.clone()),
        _ => None,
    })?;

    let sources: Vec<u32> = contents
        .iter()
        .map(|c| match c {
            Content::None => SOURCE_NONE,
            Content::Uri(_) => SOURCE_URI,
            Content::Object(_) => SOURCE_OBJECT,
        })
        .collect();
    let uris: Vec<&str> = contents
        .iter()
        .filter_map(|c| match c {
            Content::Uri(uri) => Some(uri.as_str()),
            _ => None,
        })
        .collect();
    let objects: Vec<i32> = contents
        .iter()
        .filter_map(|c| match c {
            Content::Object(r) => Some(r.value() as i32),
            _ => None,
        })
        .collect();

    let mut writer = Writer::new();
    writer.bytes(&interleave_u32(&sources));
    writer.length(uris.len());
    for uri in uris {
        writer.sized_name(uri);
    }
    writer.length(objects.len());
    writer.bytes(&encode_referents(&objects));
    writer.length(0); // external referents: never produced by this crate
    Ok(writer.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prop::{decode, PropHeader};
    use rbx_dom::Ref;

    fn decoded(type_id: u8, count: usize, payload: &[u8]) -> Vec<Option<Variant>> {
        let header = PropHeader {
            class_id: 0,
            name: "Test".to_owned(),
            type_id,
            payload,
        };
        decode(&header, count, &[])
    }

    #[test]
    fn each_tag_consumes_its_own_pool_in_order() {
        let values = vec![
            Some(Variant::Content(Content::Uri("rbxassetid://1".to_owned()))),
            Some(Variant::Content(Content::None)),
            Some(Variant::Content(Content::Object(Ref::new(7)))),
            Some(Variant::Content(Content::Uri("rbxassetid://2".to_owned()))),
        ];
        let payload = contents("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x22, 4, &payload), values);
    }
}
