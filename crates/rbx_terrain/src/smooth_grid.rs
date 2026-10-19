//! `Terrain.SmoothGrid` (and `TerrainRegion.SmoothGrid`): the voxel grid as
//! a place file stores it.
//!
//! Layout, established by decoding a real place's 66 KB blob and re-encoding
//! it byte for byte (see the ignored fixture test), and agreeing with zeux's
//! 2017 "Voxel terrain: storage" post on the run encoding:
//!
//! - Byte 0 is the format version (1), byte 1 log2 of the chunk edge (5).
//!   An empty grid is just those two bytes.
//! - Chunks follow until the end, sorted by X, then Y, then Z. Each begins
//!   with its offset from the previous chunk (the first from 0,0,0) as three
//!   32-bit two's-complement integers, big-endian, with their bytes
//!   interleaved by significance: the top byte of X, Y, Z, then the next
//!   byte of each, and so on.
//! - Then run records until the chunk's 32³ voxels are filled, X fastest,
//!   then Z, then Y. A record's lead byte holds the material slot in its low
//!   six bits; bit 6 means an occupancy byte follows (otherwise Air is empty
//!   and anything else full); bit 7 means a count byte follows, the run being
//!   count + 1 voxels. A count of 0 with bit 7 set is a one-voxel run carrying
//!   one more byte, the voxel's liquid share (see [`Cell`]).
//!
//! Other versions exist (Roblox's reader dispatches on the version byte) but
//! none has been seen in a file; they are refused, never guessed at.

use crate::grid::{ChunkKey, CHUNK, CHUNK_CELLS};
use crate::{Cell, Material, VoxelGrid};

const VERSION: u8 = 1;
const CHUNK_LOG2: u8 = 5;
const HAS_OCCUPANCY: u8 = 0x40;
const HAS_RUN: u8 = 0x80;
const MATERIAL_BITS: u8 = 0x3f;
const MAX_RUN: usize = 256;
/// Roblox's terrain extent is ±32,000 studs; chunk coordinates past it are a
/// corrupt delta, not terrain.
const FARTHEST_CHUNK: i32 = 32_000 / (CHUNK * 4) + 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SmoothGridError {
    #[error("SmoothGrid is empty, not even a header")]
    Empty,
    #[error("SmoothGrid version {0} is not one this reads (only version 1 has been seen)")]
    Version(u8),
    #[error("SmoothGrid chunk size 2^{0} is not 2^5")]
    ChunkSize(u8),
    #[error("SmoothGrid ends in the middle of a chunk")]
    Truncated,
    #[error("SmoothGrid voxel material slot {0} is past the 23 terrain materials")]
    Material(u8),
    #[error("SmoothGrid run overflows its chunk")]
    RunOverflow,
    #[error("SmoothGrid chunk ({0}, {1}, {2}) is outside the terrain's extent")]
    ChunkRange(i32, i32, i32),
    #[error("SmoothGrid repeats chunk ({0}, {1}, {2})")]
    DuplicateChunk(i32, i32, i32),
}

/// Where each chunk's voxels sit in a `SmoothGrid` blob: its key and the
/// byte range of its run records. Finding them means walking every record
/// (nothing in the format says where a chunk ends), but walking is a small
/// fraction of building the cells, and a holder of an older blob's index can
/// then decode only the chunks whose bytes differ.
pub type Index = Vec<(ChunkKey, std::ops::Range<usize>)>;

pub fn index(bytes: &[u8]) -> Result<Index, SmoothGridError> {
    let mut reader = Reader { bytes, at: 0 };
    let version = reader.byte().map_err(|_| SmoothGridError::Empty)?;
    if version != VERSION {
        return Err(SmoothGridError::Version(version));
    }
    let log2 = reader.byte()?;
    if log2 != CHUNK_LOG2 {
        return Err(SmoothGridError::ChunkSize(log2));
    }
    let mut index = Index::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut key = [0i32; 3];
    while reader.at < bytes.len() {
        let mut delta = [0u32; 3];
        for _ in 0..4 {
            for axis in &mut delta {
                *axis = (*axis << 8) | u32::from(reader.byte()?);
            }
        }
        for axis in 0..3 {
            key[axis] = key[axis].wrapping_add(delta[axis] as i32);
        }
        if key.iter().any(|v| v.unsigned_abs() > FARTHEST_CHUNK as u32) {
            return Err(SmoothGridError::ChunkRange(key[0], key[1], key[2]));
        }
        if !seen.insert(key) {
            return Err(SmoothGridError::DuplicateChunk(key[0], key[1], key[2]));
        }
        let start = reader.at;
        skip_chunk(&mut reader)?;
        index.push((
            ChunkKey {
                x: key[0],
                y: key[1],
                z: key[2],
            },
            start..reader.at,
        ));
    }
    Ok(index)
}

pub fn decode(bytes: &[u8]) -> Result<VoxelGrid, SmoothGridError> {
    let mut grid = VoxelGrid::new();
    for (key, range) in index(bytes)? {
        grid.insert_chunk(key, decode_chunk(bytes, range)?);
    }
    grid.take_dirty();
    Ok(grid)
}

/// One chunk's voxels, from a range [`index`] found.
pub fn decode_chunk(
    bytes: &[u8],
    range: std::ops::Range<usize>,
) -> Result<Box<[Cell]>, SmoothGridError> {
    let mut reader = Reader {
        bytes: bytes.get(range.clone()).ok_or(SmoothGridError::Truncated)?,
        at: 0,
    };
    read_chunk(&mut reader)
}

/// Walks one chunk's run records without building its cells, checking them
/// the way [`read_chunk`] would.
fn skip_chunk(reader: &mut Reader<'_>) -> Result<(), SmoothGridError> {
    let mut filled = 0usize;
    while filled < CHUNK_CELLS {
        let lead = reader.byte()?;
        let slot = lead & MATERIAL_BITS;
        if Material::from_slot(slot).is_none() {
            return Err(SmoothGridError::Material(slot));
        }
        if lead & HAS_OCCUPANCY != 0 {
            reader.byte()?;
        }
        let run = if lead & HAS_RUN != 0 {
            match reader.byte()? {
                0 => {
                    reader.byte()?;
                    1
                }
                count => usize::from(count) + 1,
            }
        } else {
            1
        };
        if filled + run > CHUNK_CELLS {
            return Err(SmoothGridError::RunOverflow);
        }
        filled += run;
    }
    Ok(())
}

fn read_chunk(reader: &mut Reader<'_>) -> Result<Box<[Cell]>, SmoothGridError> {
    let mut cells = Vec::with_capacity(CHUNK_CELLS);
    while cells.len() < CHUNK_CELLS {
        let lead = reader.byte()?;
        let slot = lead & MATERIAL_BITS;
        let material = Material::from_slot(slot).ok_or(SmoothGridError::Material(slot))?;
        let occupancy = if lead & HAS_OCCUPANCY != 0 {
            reader.byte()?
        } else {
            crate::cell::FULL
        };
        let (run, liquid) = if lead & HAS_RUN != 0 {
            match reader.byte()? {
                0 => (1, reader.byte()?),
                count => (usize::from(count) + 1, 0),
            }
        } else {
            (1, 0)
        };
        if cells.len() + run > CHUNK_CELLS {
            return Err(SmoothGridError::RunOverflow);
        }
        cells.resize(cells.len() + run, Cell::new(material, occupancy, liquid));
    }
    Ok(cells.into_boxed_slice())
}

/// Encodes the way Roblox does: chunks in key order, greedy runs of up to 256
/// identical voxels, occupancy written only when it differs from the
/// material's default. A real place re-encodes to its original bytes.
pub fn encode(grid: &VoxelGrid) -> Vec<u8> {
    Encoder::default().encode(grid)
}

/// [`encode`] that remembers each chunk's run records by revision, so a grid
/// encoded again after an edit only re-encodes the chunks the edit touched;
/// the rest is a copy. Keep one per grid being saved repeatedly (an editor's
/// working terrain).
#[derive(Debug, Default)]
pub struct Encoder {
    bodies: std::collections::HashMap<ChunkKey, (u64, Vec<u8>)>,
}

impl Encoder {
    pub fn encode(&mut self, grid: &VoxelGrid) -> Vec<u8> {
        let mut out = vec![VERSION, CHUNK_LOG2];
        let mut previous = [0i32; 3];
        let mut kept = std::collections::HashSet::new();
        for (key, cells, revision) in grid.revised_chunks() {
            kept.insert(key);
            let at = [key.x, key.y, key.z];
            let delta: [u32; 3] =
                std::array::from_fn(|axis| at[axis].wrapping_sub(previous[axis]) as u32);
            previous = at;
            for shift in [24, 16, 8, 0] {
                out.extend(delta.iter().map(|axis| (axis >> shift) as u8));
            }
            let body = self.bodies.entry(key).or_insert((0, Vec::new()));
            if body.0 != revision || body.1.is_empty() {
                body.1.clear();
                write_chunk(&mut body.1, cells);
                body.0 = revision;
            }
            out.extend_from_slice(&body.1);
        }
        self.bodies.retain(|key, _| kept.contains(key));
        out
    }
}

fn write_chunk(out: &mut Vec<u8>, cells: &[Cell]) {
    let mut at = 0;
    while at < cells.len() {
        let cell = cells[at];
        let mut run = 1;
        // A voxel carrying liquid needs the count-zero record to itself.
        if cell.liquid == 0 {
            while at + run < cells.len() && run < MAX_RUN && cells[at + run] == cell {
                run += 1;
            }
        }
        let default_occupancy = if cell.is_air() { 0 } else { crate::cell::FULL };
        let explicit_occupancy = cell.occupancy != default_occupancy;
        let explicit_run = run > 1 || cell.liquid != 0;
        let mut lead = cell.material.slot();
        if explicit_occupancy {
            lead |= HAS_OCCUPANCY;
        }
        if explicit_run {
            lead |= HAS_RUN;
        }
        out.push(lead);
        if explicit_occupancy {
            out.push(cell.occupancy);
        }
        if explicit_run {
            out.push((run - 1) as u8);
            if run == 1 {
                out.push(cell.liquid);
            }
        }
        at += run;
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, SmoothGridError> {
        let byte = *self.bytes.get(self.at).ok_or(SmoothGridError::Truncated)?;
        self.at += 1;
        Ok(byte)
    }
}

#[cfg(test)]
mod tests;
