use super::*;

fn full_air_chunk_runs() -> Vec<u8> {
    // 32768 Air voxels: 128 runs of 256.
    [0x80u8, 0xff].repeat(128)
}

#[test]
fn empty_grid_is_the_two_header_bytes() {
    assert_eq!(encode(&VoxelGrid::new()), vec![1, 5]);
    assert!(decode(&[1, 5]).unwrap().is_empty());
}

#[test]
fn chunk_offsets_are_interleaved_big_endian() {
    // The first chunk header of a real place: (-2, -1, 0).
    let mut grid = VoxelGrid::new();
    grid.set([-64, -32, 0], Cell::full(Material::Rock));
    let bytes = encode(&grid);
    assert_eq!(
        &bytes[2..14],
        &[0xff, 0xff, 0x00, 0xff, 0xff, 0x00, 0xff, 0xff, 0x00, 0xfe, 0xff, 0x00]
    );
    // One Rock voxel, then 32767 Air: 127 full runs and one of 255.
    assert_eq!(&bytes[14..17], &[Material::Rock.slot(), 0x80, 0xff]);
    assert_eq!(decode(&bytes).unwrap(), grid);
}

#[test]
fn later_chunks_store_their_offset_from_the_previous_one() {
    let mut grid = VoxelGrid::new();
    grid.set([0, 0, 0], Cell::full(Material::Grass));
    grid.set([0, 32, 0], Cell::full(Material::Grass));
    let bytes = encode(&grid);
    let second = 2 + 12 + 3 + 254;
    // (0,0,0) then (0,1,0): only Y's low byte is set.
    assert_eq!(
        &bytes[second..second + 12],
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]
    );
    assert_eq!(decode(&bytes).unwrap(), grid);
}

#[test]
fn partial_occupancy_and_liquid_round_trip() {
    let mut grid = VoxelGrid::new();
    grid.set([1, 2, 3], Cell::new(Material::Sand, 100, 0));
    grid.set([2, 2, 3], Cell::new(Material::Sand, 100, 0));
    grid.set([3, 2, 3], Cell::new(Material::Rock, 90, 200));
    grid.set([4, 2, 3], Cell::new(Material::Water, 255, 0));
    let bytes = encode(&grid);
    // The two Sand voxels share a run with an explicit occupancy byte...
    assert!(bytes
        .windows(3)
        .any(|w| w == [Material::Sand.slot() | 0xc0, 100, 1]));
    // ...and the Rock voxel is a count-zero record carrying its liquid byte.
    assert!(bytes
        .windows(4)
        .any(|w| w == [Material::Rock.slot() | 0xc0, 90, 0, 200]));
    assert_eq!(decode(&bytes).unwrap(), grid);
}

#[test]
fn reader_normalizes_air_and_full_voxels() {
    let mut bytes = vec![1, 5];
    bytes.extend([0; 12]);
    // Air with an explicit occupancy, then a full Grass voxel with a liquid
    // byte, then the rest Air.
    bytes.extend([0x40, 77, Material::Grass.slot() | 0x80, 0, 9]);
    bytes.extend([0x80, 0xfd]);
    bytes.extend([0x80, 0xff].repeat(127));
    let grid = decode(&bytes).unwrap();
    assert_eq!(grid.get([0, 0, 0]), Cell::AIR);
    assert_eq!(grid.get([1, 0, 0]), Cell::full(Material::Grass));
}

#[test]
fn all_air_chunks_are_dropped() {
    let mut bytes = vec![1, 5];
    bytes.extend([0; 12]);
    bytes.extend(full_air_chunk_runs());
    assert!(decode(&bytes).unwrap().is_empty());
}

#[test]
fn malformed_blobs_are_refused() {
    assert_eq!(decode(&[]), Err(SmoothGridError::Empty));
    assert_eq!(decode(&[2, 5]), Err(SmoothGridError::Version(2)));
    assert_eq!(decode(&[1, 4]), Err(SmoothGridError::ChunkSize(4)));
    assert_eq!(decode(&[1, 5, 0, 0]), Err(SmoothGridError::Truncated));
    let mut bad = vec![1, 5];
    bad.extend([0; 12]);
    bad.push(23);
    assert_eq!(decode(&bad), Err(SmoothGridError::Material(23)));
    // 127 full runs and one of 255 leave one voxel; a 2-voxel run overflows.
    let mut over = vec![1, 5];
    over.extend([0; 12]);
    over.extend([0x80, 0xff].repeat(127));
    over.extend([0x80, 0xfe, 0x80, 0x01]);
    assert_eq!(decode(&over), Err(SmoothGridError::RunOverflow));
}

#[test]
fn far_chunks_and_repeats_are_refused() {
    let mut far = vec![1, 5];
    far.extend([0, 0, 0, 0, 0, 0, 0x01, 0, 0, 0, 0, 0]);
    far.extend(full_air_chunk_runs());
    assert!(matches!(decode(&far), Err(SmoothGridError::ChunkRange(..))));
    // A delta landing on i32::MIN, whose `abs` overflows.
    let mut min = vec![1, 5];
    min.extend([0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    min.extend(full_air_chunk_runs());
    assert!(matches!(decode(&min), Err(SmoothGridError::ChunkRange(..))));
    let mut twice = vec![1, 5];
    twice.extend([0; 12]);
    twice.extend(full_air_chunk_runs());
    twice.extend([0; 12]);
    twice.extend(full_air_chunk_runs());
    assert_eq!(
        decode(&twice),
        Err(SmoothGridError::DuplicateChunk(0, 0, 0))
    );
}

#[test]
fn varied_grid_round_trips() {
    let mut grid = VoxelGrid::new();
    let mut seed = 0x2545_f491u32;
    for _ in 0..4000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let voxel = [
            (seed % 97) as i32 - 48,
            (seed / 97 % 61) as i32 - 30,
            (seed / 5917 % 89) as i32 - 44,
        ];
        let material = Material::from_slot((seed >> 24) as u8 % 23).unwrap();
        grid.set(
            voxel,
            Cell::new(material, (seed >> 8) as u8, (seed >> 16) as u8),
        );
    }
    let bytes = encode(&grid);
    let back = decode(&bytes).unwrap();
    assert_eq!(back, grid);
    assert_eq!(encode(&back), bytes);
}

#[test]
fn the_index_finds_each_chunk_and_its_bytes_decode_alone() {
    let mut grid = VoxelGrid::new();
    grid.set([1, 2, 3], Cell::new(Material::Rock, 90, 7));
    grid.set([40, 2, 3], Cell::full(Material::Sand));
    let bytes = encode(&grid);
    let index = index(&bytes).unwrap();
    assert_eq!(index.len(), 2);
    let (key, range) = index[1].clone();
    assert_eq!(key, ChunkKey { x: 1, y: 0, z: 0 });
    let cells = decode_chunk(&bytes, range).unwrap();
    assert_eq!(
        cells[crate::grid::index_in_chunk([40, 2, 3])],
        Cell::full(Material::Sand)
    );
    assert!(super::index(&bytes[..bytes.len() - 1]).is_err());
}

#[test]
fn the_encoder_reuses_untouched_chunks_and_matches_a_fresh_encode() {
    let mut grid = VoxelGrid::new();
    for x in 0..100 {
        grid.set([x, 0, 0], Cell::full(Material::Grass));
    }
    let mut encoder = Encoder::default();
    assert_eq!(encoder.encode(&grid), encode(&grid));
    grid.set([5, 0, 0], Cell::full(Material::Snow));
    grid.set([99, 0, 0], Cell::AIR);
    assert_eq!(encoder.encode(&grid), encode(&grid));
    // Emptying a whole chunk drops it from the output, as a fresh encode does.
    for x in 96..99 {
        grid.set([x, 0, 0], Cell::AIR);
    }
    assert_eq!(encoder.encode(&grid), encode(&grid));
    assert_eq!(decode(&encoder.encode(&grid)).unwrap(), grid);
}
