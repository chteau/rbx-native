//! Decoder for the Content property type.

use rbx_dom::{Content, Ref, Variant};

use super::PropValues;
use crate::codec::Reader;
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
    // TODO: the tags are read as an untransformed Enum array, per the written spec.
    // rbx-dom's implementation instead zigzags them, and the two disagree for every
    // value but 0 — which is the only one either fixture contains, so the bytes
    // cannot settle it. A file with a real Uri or Object Content would.
    let sources = reader.interleaved_u32(count)?;

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
            out.extend(sources.iter().map(|s| s.to_be_bytes()[column]));
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

    // TODO: neither fixture populates the URI or referent pools (every Content in
    // them is None), so these two branches are only covered synthetically.
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
