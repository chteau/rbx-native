//! Content encoding, the encode counterpart of `chunks::prop::content`.

use rbx_dom::{Content, Ref, Variant, WeakDom};

use crate::codec::{encode_referents, interleave_zigzag_i32};
use crate::serialize::prop::map_dense;
use crate::serialize::writer::Writer;
use crate::serialize::SerializeError;

const SOURCE_NONE: i32 = 0;
const SOURCE_URI: i32 = 1;
const SOURCE_OBJECT: i32 = 2;

/// The binary format's null referent (rbx-dom `docs/binary.md`, Referent).
const NULL_REFERENT: i32 = -1;

/// Stands in for an object that is not in the file being written, so the
/// encoder writes [`NULL_REFERENT`] for it — what rbx-dom's writer does for a
/// target it has no referent for, and what both readers turn back into an
/// empty Content. No instance can hold it: a file's referents are
/// non-negative `i32`s.
const DANGLING: Ref = Ref::new(u32::MAX);

/// Points every `Object` content whose target is not in `dom` at
/// [`DANGLING`]. A referent the file does not define would otherwise be
/// written as-is and resolve to nothing, or to the wrong instance, on load.
pub(crate) fn null_dangling_objects(dom: &WeakDom, values: &mut [Option<Variant>]) {
    for value in values.iter_mut() {
        if let Some(Variant::Content(Content::Object(target))) = value {
            if dom.get(*target).is_none() {
                *target = DANGLING;
            }
        }
    }
}

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

    // Zigzagged, the way Studio writes them (see `chunks::prop::content`).
    let sources: Vec<i32> = contents
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
    // In instance order, as rbx-dom's writer emits them and as Studio orders
    // the URI pool beside it (rbx-test-files `imagelabel-content`).
    let objects: Vec<i32> = contents
        .iter()
        .filter_map(|c| match c {
            Content::Object(r) if *r == DANGLING => Some(NULL_REFERENT),
            Content::Object(r) => Some(r.value() as i32),
            _ => None,
        })
        .collect();

    let mut writer = Writer::new();
    writer.bytes(&interleave_zigzag_i32(&sources));
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

    // The object pool is in instance order: the first Object instance's
    // referent comes first. rbx-dom's writer agrees; its reader pops the
    // pool from the back, which would swap these two — see
    // `chunks::prop::content::contents` for why that is not followed.
    #[test]
    fn two_objects_keep_their_instances_order() {
        let object = |id| Some(Variant::Content(Content::Object(Ref::new(id))));
        let values = vec![object(9), object(4)];
        let payload = contents("Test", "Prop", &values).unwrap();

        // Two Object tags (zigzagged to 4), no URIs, two referents: 9, then
        // the delta 4 - 9 = -5, zigzagged to 18 and 9; no external ones.
        #[rustfmt::skip]
        let expected = [
            0, 0, 0, 0, 0, 0, 4, 4,
            0, 0, 0, 0,
            2, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 18, 9,
            0, 0, 0, 0,
        ];
        assert_eq!(payload, expected);
        assert_eq!(decoded(0x22, 2, &payload), values);
    }

    #[test]
    fn an_object_outside_the_file_is_written_as_the_null_referent() {
        let mut dom = WeakDom::new();
        let inside = dom.new_instance("EditableImage", "Inside", None);
        let mut values = vec![
            Some(Variant::Content(Content::Object(inside))),
            Some(Variant::Content(Content::Object(Ref::new(4242)))),
        ];
        null_dangling_objects(&dom, &mut values);
        let payload = contents("Test", "Prop", &values).unwrap();

        // The second referent's delta is -1 - inside: decoding it lands on -1.
        assert_eq!(
            decoded(0x22, 2, &payload),
            vec![
                Some(Variant::Content(Content::Object(inside))),
                Some(Variant::Content(Content::None)),
            ]
        );
    }

    // rbx-test-files' `imagelabel-content/binary.rbxm`, saved by Studio
    // 0.663: the three ImageLabels' ImageContent column must come out byte
    // for byte as Studio wrote it.
    #[test]
    fn a_uri_column_is_written_the_way_studio_writes_it() {
        let placeholder = "rbxasset://textures/ui/GuiImagePlaceholder.png";
        let spawn = "rbxasset://textures/SpawnLocation.png";
        let uri = |s: &str| Some(Variant::Content(Content::Uri(s.to_owned())));
        let values = vec![
            uri(placeholder),
            uri(spawn),
            Some(Variant::Content(Content::None)),
        ];

        let mut studio = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 0, 2, 0, 0, 0];
        for s in [placeholder, spawn] {
            studio.extend_from_slice(&(s.len() as u32).to_le_bytes());
            studio.extend_from_slice(s.as_bytes());
        }
        studio.extend_from_slice(&[0; 8]);

        assert_eq!(
            contents("ImageLabel", "ImageContent", &values).unwrap(),
            studio
        );
    }
}
