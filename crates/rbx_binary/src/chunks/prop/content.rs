//! Decoder for the Content property type.

use rbx_dom::{Content, Ref, Variant};

use super::PropValues;
use crate::codec::{zigzag_i32, Reader};
use crate::error::BinaryError;

const SOURCE_NONE: u32 = 0;
const SOURCE_URI: u32 = 1;
const SOURCE_OBJECT: u32 = 2;

/// Reads a Content property array.
///
/// The payload is a column of source-type tags followed by one pool per source kind
/// (URIs, then in-file referents, then external referents). Each tag consumes the
/// next entry of its own pool, so the pools are shorter than the instance count.
pub(super) fn contents(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    // The tags are zigzagged `i32`s, not the plain `Enum`s rbx-dom's
    // `docs/binary.md` types them as: Studio 0.663 writes a Uri tag as raw `2`
    // (rbx-test-files `models/imagelabel-content/binary.rbxm`, whose two Uri
    // values and empty object pool fit no other reading), and rbx-dom's own
    // reader and writer zigzag them too.
    let sources: Vec<u32> = reader
        .interleaved_u32(count)?
        .into_iter()
        .map(|raw| zigzag_i32(raw) as u32)
        .collect();

    let uris = (0..reader.length()?)
        .map(|_| reader.sized_name())
        .collect::<Result<Vec<_>, _>>()?;

    let object_count = reader.length()?;
    let objects = reader.referents(object_count)?;

    // External referents point at instances in *another* document, so they can
    // never be resolved from this file; they are read only to stay aligned.
    let external_count = reader.length()?;
    reader.referents(external_count)?;

    let mut uris = uris.into_iter();
    let mut objects = objects.into_iter();

    sources
        .into_iter()
        .map(|source| {
            let content = match source {
                SOURCE_NONE => Content::None,
                SOURCE_URI => Content::Uri(next(&mut uris)?),
                SOURCE_OBJECT => match u32::try_from(next(&mut objects)?) {
                    Ok(id) => Content::Object(Ref::new(id)),
                    // -1 is the null referent, which is an empty Content rather
                    // than a reference the DOM failed to resolve.
                    Err(_) => Content::None,
                },
                other => return Err(BinaryError::UnknownContentSource(other)),
            };
            Ok(Some(Variant::Content(content)))
        })
        .collect()
}

// A pool shorter than its tags announce means the counts and the tag column
// disagree; guessing a value would hide real corruption.
fn next<T>(pool: &mut impl Iterator<Item = T>) -> Result<T, BinaryError> {
    pool.next().ok_or(BinaryError::ContentPoolExhausted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(sources: &[u32], uris: &[&str], objects: &[i32]) -> Vec<u8> {
        let mut out = Vec::new();
        for column in 0..4 {
            // Zigzagged, as Studio writes them (see `contents`).
            out.extend(sources.iter().map(|s| (s << 1).to_be_bytes()[column]));
        }

        out.extend_from_slice(&(uris.len() as i32).to_le_bytes());
        for uri in uris {
            out.extend_from_slice(&(uri.len() as i32).to_le_bytes());
            out.extend_from_slice(uri.as_bytes());
        }

        out.extend_from_slice(&(objects.len() as i32).to_le_bytes());
        let mut previous = 0i32;
        for &object in objects {
            // Referent arrays are delta then zigzag encoded.
            let delta = object - previous;
            previous = object;
            out.extend_from_slice(&((delta << 1) ^ (delta >> 31)).to_be_bytes());
        }

        out.extend_from_slice(&0i32.to_le_bytes());
        out
    }

    // Decal.EmissiveMaskContent from TestPlace.rbxl: sixteen zero bytes, i.e. one
    // empty Content and three empty pools.
    #[test]
    fn all_zero_payload_is_a_single_empty_content() {
        let values = contents(&mut Reader::new(&[0; 16]), 1).unwrap();
        assert_eq!(values[0], Some(Variant::Content(Content::None)));
    }

    // The PROP payload of rbx-test-files' `imagelabel-content/binary.rbxm`
    // (Studio 0.663), copied byte for byte: three ImageLabels' ImageContent,
    // raw tags 2, 2, 0 and two URIs.
    #[test]
    fn a_studio_written_uri_column_decodes_as_uris() {
        let mut data = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 0];
        data.extend_from_slice(&2u32.to_le_bytes());
        for uri in [
            "rbxasset://textures/ui/GuiImagePlaceholder.png",
            "rbxasset://textures/SpawnLocation.png",
        ] {
            data.extend_from_slice(&(uri.len() as u32).to_le_bytes());
            data.extend_from_slice(uri.as_bytes());
        }
        data.extend_from_slice(&[0; 8]);

        let values = contents(&mut Reader::new(&data), 3).unwrap();
        let uri = |s: &str| Some(Variant::Content(Content::Uri(s.to_owned())));
        assert_eq!(
            values,
            vec![
                uri("rbxasset://textures/ui/GuiImagePlaceholder.png"),
                uri("rbxasset://textures/SpawnLocation.png"),
                Some(Variant::Content(Content::None)),
            ]
        );
    }

    // No real file populates the referent pool (rbx-dom's `docs/binary.md`:
    // "ObjectRefs are not populated outside of copy-and-pasting within
    // Studio"), so the Object branch is covered synthetically.
    #[test]
    fn each_tag_consumes_its_own_pool_in_order() {
        let data = payload(
            &[SOURCE_URI, SOURCE_NONE, SOURCE_OBJECT, SOURCE_URI],
            &["rbxassetid://1", "rbxassetid://2"],
            &[7],
        );

        let values = contents(&mut Reader::new(&data), 4).unwrap();

        assert_eq!(
            values,
            vec![
                Some(Variant::Content(Content::Uri("rbxassetid://1".to_owned()))),
                Some(Variant::Content(Content::None)),
                Some(Variant::Content(Content::Object(Ref::new(7)))),
                Some(Variant::Content(Content::Uri("rbxassetid://2".to_owned()))),
            ]
        );
    }

    #[test]
    fn a_null_object_referent_is_an_empty_content() {
        let data = payload(&[SOURCE_OBJECT], &[], &[-1]);
        let values = contents(&mut Reader::new(&data), 1).unwrap();

        assert_eq!(values[0], Some(Variant::Content(Content::None)));
    }

    #[test]
    fn an_undefined_source_type_is_rejected() {
        let data = payload(&[3], &[], &[]);
        assert!(matches!(
            contents(&mut Reader::new(&data), 1),
            Err(BinaryError::UnknownContentSource(3))
        ));
    }

    #[test]
    fn a_tag_without_a_pool_entry_is_rejected() {
        let data = payload(&[SOURCE_URI], &[], &[]);
        assert!(contents(&mut Reader::new(&data), 1).is_err());
    }
}
