//! A union's collision: the convex pieces Studio decomposed it into when
//! it made the union, kept in its `PhysicalConfigData`. A hollow union (a
//! shell, an arch, a tunnel) collides and touches only where it has
//! material, not over its whole `Size` box.
//!
//! The layout below is read off the files Studio writes (no format is
//! published): `CSGPHS`, a little-endian `u32` version, then for versions 6
//! and 7 a header of mass data and the pieces back to back, each:
//!
//! - `u32 16` and four floats (zero in every file seen),
//! - `u32 16` and four floats (an identity quaternion in every file seen),
//! - `u32` float count, `u32 4`, then that many floats: the vertices, in the
//!   union's own space as it was made (against `InitialSize`),
//! - `u32` index count, then that many `u32`s: the hull's triangles.
//!
//! Version 0, `CSGPHS` + four zero bytes + `BLOCK`, is a union that
//! collides as its box; so is anything this can't read.

use std::sync::Arc;

/// One convex piece's vertices.
pub(crate) type Hull = Vec<[f32; 3]>;

const MAGIC: &[u8] = b"CSGPHS";

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Option<u32> {
        let word = self.bytes.get(self.at..self.at + 4)?;
        self.at += 4;
        Some(u32::from_le_bytes(word.try_into().ok()?))
    }

    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }
}

/// One piece from `at`, and where the next one starts.
fn piece(bytes: &[u8], at: usize) -> Option<(Hull, usize)> {
    let mut r = Reader { bytes, at };
    for _ in 0..2 {
        if r.u32()? != 16 {
            return None;
        }
        for _ in 0..4 {
            r.f32()?;
        }
    }
    let floats = r.u32()? as usize;
    if floats == 0 || !floats.is_multiple_of(3) || r.u32()? != 4 || floats * 4 > bytes.len() {
        return None;
    }
    let mut vertices = Vec::with_capacity(floats / 3);
    for _ in 0..floats / 3 {
        let v = [r.f32()?, r.f32()?, r.f32()?];
        if !v.iter().all(|c| c.is_finite()) {
            return None;
        }
        vertices.push(v);
    }
    let indices = r.u32()? as usize;
    if !indices.is_multiple_of(3) || indices * 4 > bytes.len() {
        return None;
    }
    for _ in 0..indices {
        if r.u32()? as usize >= vertices.len() {
            return None;
        }
    }
    Some((vertices, r.at))
}

/// The pieces of a `PhysicalConfigData`, `None` for a box (or a blob this
/// can't read whole).
pub(crate) fn hulls(bytes: &[u8]) -> Option<Arc<Vec<Hull>>> {
    if !bytes.starts_with(MAGIC) {
        return None;
    }
    let version = u32::from_le_bytes(bytes.get(6..10)?.try_into().ok()?);
    if !matches!(version, 6 | 7) {
        return None;
    }
    // The header's length differs by a byte between the versions seen;
    // the first piece is where one parses.
    let first = (10..80).find(|&at| piece(bytes, at).is_some())?;
    let mut pieces = Vec::new();
    let mut at = first;
    while at < bytes.len() {
        let (hull, next) = piece(bytes, at)?;
        pieces.push(hull);
        at = next;
    }
    (!pieces.is_empty()).then(|| Arc::new(pieces))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(version: u32, header: usize, pieces: &[Hull]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend(version.to_le_bytes());
        out.extend(std::iter::repeat_n(0x41u8, header));
        let word = |out: &mut Vec<u8>, w: u32| out.extend(w.to_le_bytes());
        for hull in pieces {
            word(&mut out, 16);
            for _ in 0..4 {
                word(&mut out, 0f32.to_bits());
            }
            word(&mut out, 16);
            for q in [0.0f32, 0.0, 0.0, 1.0] {
                word(&mut out, q.to_bits());
            }
            word(&mut out, (hull.len() * 3) as u32);
            word(&mut out, 4);
            for v in hull {
                for c in v {
                    word(&mut out, c.to_bits());
                }
            }
            word(&mut out, 3);
            for i in 0..3 {
                word(&mut out, i);
            }
        }
        out
    }

    fn tetra(at: f32) -> Hull {
        vec![
            [at, 0.0, 0.0],
            [at + 1.0, 0.0, 0.0],
            [at, 1.0, 0.0],
            [at, 0.0, 1.0],
        ]
    }

    #[test]
    fn reads_every_piece_of_both_versions() {
        for (version, header) in [(7, 41), (6, 40)] {
            let found = hulls(&blob(version, header, &[tetra(0.0), tetra(5.0)])).unwrap();
            assert_eq!(found.len(), 2);
            assert_eq!(found[1][1], [6.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn a_block_or_a_cut_blob_is_a_box() {
        let mut block = MAGIC.to_vec();
        block.extend([0, 0, 0, 0]);
        block.extend(b"BLOCK");
        assert!(hulls(&block).is_none());
        let whole = blob(7, 41, &[tetra(0.0)]);
        assert!(hulls(&whole[..whole.len() - 6]).is_none());
    }
}
