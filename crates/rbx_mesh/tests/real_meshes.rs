//! Integration tests over real mesh files downloaded from the Roblox CDN.
//!
//! The fixtures are deliberately out of the repository (they are third-party
//! assets), so these tests are `#[ignore]`d and read the directory named by
//! `RBX_MESH_FIXTURES`, one file per asset id:
//!
//! ```text
//! RBX_MESH_FIXTURES=/path/to/meshes cargo test -p rbx_mesh -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use rbx_mesh::{parse, Mesh};

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(
        std::env::var("RBX_MESH_FIXTURES")
            .expect("set RBX_MESH_FIXTURES to the directory holding the mesh fixtures"),
    );

    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("failed to list {dir:?}: {err}"))
        .map(|entry| entry.expect("failed to read a directory entry").path())
        .filter(|path| path.is_file())
        .map(|path| {
            let name = path
                .file_name()
                .expect("a file must have a name")
                .to_string_lossy()
                .into_owned();
            let bytes =
                std::fs::read(&path).unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));
            (name, bytes)
        })
        .collect();

    assert!(!files.is_empty(), "no fixtures found in {dir:?}");
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn parsed() -> Vec<(String, Mesh)> {
    fixtures()
        .into_iter()
        .map(|(name, bytes)| {
            let mesh =
                parse(&bytes).unwrap_or_else(|err| panic!("failed to parse fixture {name}: {err}"));
            (name, mesh)
        })
        .collect()
}

#[test]
#[ignore = "needs RBX_MESH_FIXTURES"]
fn every_fixture_parses_into_whole_triangles_addressing_real_vertices() {
    for (name, mesh) in parsed() {
        assert!(!mesh.vertices.is_empty(), "{name}: parsed to zero vertices");
        assert!(
            mesh.indices.len() % 3 == 0,
            "{name}: {} indices is not a whole number of triangles",
            mesh.indices.len()
        );
        assert!(
            !mesh.indices.is_empty(),
            "{name}: LOD 0 came out empty, the LOD table was probably believed too hard"
        );
        for &index in &mesh.indices {
            assert!(
                (index as usize) < mesh.vertices.len(),
                "{name}: index {index} exceeds {} vertices",
                mesh.vertices.len()
            );
        }
        assert_eq!(
            mesh.lod0().len(),
            mesh.indices.len(),
            "{name}: lod0() disagrees with indices"
        );
    }
}

#[test]
#[ignore = "needs RBX_MESH_FIXTURES"]
fn every_declared_lod_range_stays_inside_the_face_list() {
    for (name, mesh) in parsed() {
        let Some(first) = mesh.lods.first() else {
            continue;
        };
        assert_eq!(
            first.len() * 3,
            mesh.indices.len(),
            "{name}: LOD 0 range {first:?} does not match the retained indices"
        );
        for pair in mesh.lods.windows(2) {
            assert_eq!(
                pair[0].end, pair[1].start,
                "{name}: LOD ranges are not contiguous: {:?} then {:?}",
                pair[0], pair[1]
            );
        }
    }
}

#[test]
#[ignore = "needs RBX_MESH_FIXTURES"]
fn lod0_normals_are_unit_length_or_degenerate() {
    // Only the vertices LOD 0 actually references are asserted on. Roblox's LOD
    // simplifier leaves the decimated tail unnormalized -- fixture 4845862166
    // (v3.00) has 111 such vertices, all of them exclusive to LOD 1 and LOD 2 --
    // and hand-authored v1 meshes were never normalized at all (1374148 reaches
    // 5.5). Both are real bytes, so the parser hands them back untouched and this
    // test reports rather than fails on them.
    for (name, mesh) in parsed() {
        let mut worst_used = 0.0f32;
        let mut worst_any = 0.0f32;
        let used: std::collections::HashSet<u32> = mesh.indices.iter().copied().collect();

        for (index, vertex) in mesh.vertices.iter().enumerate() {
            let length = vertex
                .normal
                .iter()
                .map(|axis| axis * axis)
                .sum::<f32>()
                .sqrt();
            assert!(
                length.is_finite(),
                "{name}: a normal is not finite: {:?}",
                vertex.normal
            );

            // A zero normal is a legitimate "unshaded" marker, not a misparse.
            let error = if length == 0.0 {
                0.0
            } else {
                (length - 1.0).abs()
            };
            worst_any = worst_any.max(error);
            if used.contains(&(index as u32)) {
                worst_used = worst_used.max(error);
            }
        }

        if worst_any > 1e-3 {
            println!("{name}: normals off unit length by up to {worst_any:.4} outside LOD 0");
        }
        if mesh.version.0 == 1 {
            println!("{name}: v1, normals not asserted (off by up to {worst_any:.4})");
            continue;
        }
        assert!(
            worst_used < 1e-3,
            "{name}: a LOD 0 normal is {worst_used} away from unit length"
        );
    }
}

#[test]
#[ignore = "needs RBX_MESH_FIXTURES"]
fn uvs_land_in_a_plausible_texture_range() {
    // The parser flips v only for v1, so if that branch were wrong (or applied to
    // the wrong versions) the v1 meshes would sit mirrored against the others.
    // Tiling is legal, hence the generous bound rather than a strict 0..1.
    for (name, mesh) in parsed() {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for vertex in &mesh.vertices {
            for coord in vertex.uv {
                assert!(coord.is_finite(), "{name}: a uv is not finite");
                lo = lo.min(coord);
                hi = hi.max(coord);
            }
        }
        assert!(
            lo > -8.0 && hi < 8.0,
            "{name}: uv range [{lo}, {hi}] does not look like texture coordinates"
        );
        println!("{name}: uv range [{lo:.3}, {hi:.3}]");
    }
}

#[test]
#[ignore = "needs RBX_MESH_FIXTURES"]
fn bounds_are_finite_and_of_a_plausible_stud_scale() {
    // A mesh is authored around the origin and scaled by the MeshPart at runtime,
    // so its own extent stays within a few hundred studs. Anything past that (or
    // any NaN/inf) means positions were read at the wrong offset.
    for (name, mesh) in parsed() {
        for axis in 0..3 {
            assert!(
                mesh.bounds.min[axis].is_finite() && mesh.bounds.max[axis].is_finite(),
                "{name}: bounds are not finite: {:?}",
                mesh.bounds
            );
            assert!(
                mesh.bounds.min[axis] <= mesh.bounds.max[axis],
                "{name}: inverted bounds on axis {axis}: {:?}",
                mesh.bounds
            );
        }

        let size = mesh.bounds.size();
        let largest = size.iter().copied().fold(0.0f32, f32::max);
        assert!(
            largest > 1e-4 && largest < 1000.0,
            "{name}: implausible extent {size:?}"
        );
    }
}

#[test]
#[ignore = "needs RBX_MESH_FIXTURES"]
fn a_hero_mesh_asset_is_a_handful_of_studs_across() {
    // Asset 10095349003 is placed with Scale 0.6 in real places, so its extent
    // must read as a couple of studs, not a couple of thousand.
    let (_, mesh) = parsed()
        .into_iter()
        .find(|(name, _)| name == "10095349003")
        .expect("fixture 10095349003 is missing");

    assert_eq!(mesh.version, (4, 1));
    assert_eq!(mesh.vertices.len(), 6521);
    assert_eq!(mesh.lods.len(), 5);
    assert_eq!(mesh.indices.len(), 4894 * 3);

    let size = mesh.bounds.size();
    assert!(
        size.iter().all(|&axis| axis > 0.4 && axis < 3.0),
        "expected a few studs across, got {size:?}"
    );
}

#[test]
#[ignore = "needs RBX_MESH_FIXTURES"]
fn print_the_fixture_table() {
    println!(
        "\n{:<14} {:>7} {:>8} {:>8} {:>5} {:>7}  extent",
        "id", "version", "verts", "faces", "lods", "tris0"
    );
    let mut by_version: std::collections::BTreeMap<(u8, u8), usize> = Default::default();

    for (name, mesh) in parsed() {
        *by_version.entry(mesh.version).or_default() += 1;
        let size = mesh.bounds.size();
        println!(
            "{:<14} {:>4}.{:02} {:>8} {:>8} {:>5} {:>7}  ({:.2}, {:.2}, {:.2})",
            name,
            mesh.version.0,
            mesh.version.1,
            mesh.vertices.len(),
            mesh.lods
                .last()
                .map_or(mesh.triangle_count(), |r| r.end as usize),
            mesh.lods.len(),
            mesh.triangle_count(),
            size[0],
            size[1],
            size[2],
        );
    }

    println!("\nversions seen:");
    for ((major, minor), count) in by_version {
        println!("  {major}.{minor:02} x {count}");
    }
}

#[test]
#[ignore = "needs RBX_MESH_FIXTURES"]
fn no_truncation_or_mutation_of_a_real_file_ever_panics() {
    for (_, bytes) in fixtures() {
        let step = (bytes.len() / 64).max(1);
        for len in (0..bytes.len()).step_by(step) {
            let _ = parse(&bytes[..len]);
        }

        // xorshift* keeps the mutation deterministic without a dev-dependency.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..64 {
            let mut damaged = bytes.clone();
            for _ in 0..8 {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                let index =
                    (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as usize % damaged.len();
                damaged[index] = (state >> 8) as u8;
            }
            let _ = parse(&damaged);
        }
    }
}
