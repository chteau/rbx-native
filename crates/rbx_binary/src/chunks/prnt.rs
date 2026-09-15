//! PRNT (parent) chunk parsing.
//!
//! Declares parent-child relationships between instances.

use crate::codec::Reader;
use crate::error::BinaryError;

// A parent of -1 means "no parent": the instance sits at the root of the file.
pub(crate) const NO_PARENT: i32 = -1;

/// A parent-child relationship between two instance IDs.
pub(crate) struct ParentLink {
    pub(crate) child: i32,
    pub(crate) parent: i32,
}

/// Parses a PRNT chunk payload.
///
/// Layout: u8 version, i32 count, then two referent arrays (children, then parents)
/// that are read pairwise by index.
pub(crate) fn parse(data: &[u8]) -> Result<Vec<ParentLink>, BinaryError> {
    let mut reader = Reader::new(data);

    let _version = reader.u8()?;
    let count = reader.length()?;
    let children = reader.referents(count)?;
    let parents = reader.referents(count)?;

    Ok(children
        .into_iter()
        .zip(parents)
        .map(|(child, parent)| ParentLink { child, parent })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_children_with_parents() {
        let mut data = vec![0u8];
        data.extend_from_slice(&2i32.to_le_bytes());
        // children 1, 2
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 2, 2]);
        // parents 0, -1 -> zigzag(0) = 0 then delta zigzag(1) = -1
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);

        let links = parse(&data).unwrap();

        assert_eq!(links.len(), 2);
        assert_eq!((links[0].child, links[0].parent), (1, 0));
        assert_eq!((links[1].child, links[1].parent), (2, NO_PARENT));
    }

    #[test]
    fn truncated_chunk_errors() {
        assert!(parse(&[0u8, 5, 0, 0, 0]).is_err());
    }
}
