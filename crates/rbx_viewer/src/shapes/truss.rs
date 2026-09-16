//! The `TrussPart` lattice girder: a 2x2-stud square tube along local X, four
//! continuous corner rails plus repeating cross-bracing, one repeat per real
//! 2-stud length (`Enum.Style`'s three patterns). Roblox restricts a real
//! `TrussPart.Size` to `2*2*n` studs (`n` a multiple of 2), which is why a
//! whole number of repeats always fits exactly.

use glam::Vec3;

use super::MeshData;

/// `Enum.Style`: AlternatingSupports=0 (default), BridgeStyleSupports=1,
/// NoSupports=2.
// Every variant ends in "Supports" because that is Enum.Style's own naming
// (AlternatingSupports, BridgeStyleSupports, NoSupports) — matching it verbatim
// is more useful here than a shorter name clippy would prefer.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrussStyle {
    #[default]
    AlternatingSupports,
    BridgeStyleSupports,
    NoSupports,
}

/// Which real local axis a truss's long axis ends up on. [`truss`] always
/// bakes it as X; [`oriented`] rotates that onto whichever axis
/// `scene::shape` finds to be the part's largest `size` dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrussAxis {
    X,
    Y,
    Z,
}

// A 0.25-stud bar in a 2-stud-wide cross-section is 0.125 wide in this unit
// mesh (which spans one stud as 0.5), so its half-width is 0.0625.
const RAIL_HALF: f32 = 0.0625;
// Rail centers sit inward from the +-0.5 cross-section edge by one half-width,
// so each rail's outer face lands exactly flush with the 2x2 square.
const RAIL_OFFSET: f32 = 0.5 - RAIL_HALF;

type CornerAt = fn(f32) -> Vec3;

// The tube's four side faces, each as the pair of corner-rail positions (at a
// given x) that its diagonal braces connect.
const FACES: [(CornerAt, CornerAt); 4] = [
    (
        |x| Vec3::new(x, RAIL_OFFSET, RAIL_OFFSET),
        |x| Vec3::new(x, RAIL_OFFSET, -RAIL_OFFSET),
    ),
    (
        |x| Vec3::new(x, -RAIL_OFFSET, RAIL_OFFSET),
        |x| Vec3::new(x, -RAIL_OFFSET, -RAIL_OFFSET),
    ),
    (
        |x| Vec3::new(x, RAIL_OFFSET, RAIL_OFFSET),
        |x| Vec3::new(x, -RAIL_OFFSET, RAIL_OFFSET),
    ),
    (
        |x| Vec3::new(x, RAIL_OFFSET, -RAIL_OFFSET),
        |x| Vec3::new(x, -RAIL_OFFSET, -RAIL_OFFSET),
    ),
];

/// Builds a truss lattice fitting `[-0.5, 0.5]^3`, long axis X: four rails
/// running the full length, plus `segments` repeats of `style`'s bracing.
pub(crate) fn truss(segments: u32, style: TrussStyle) -> MeshData {
    let segments = segments.max(1);
    let mut mesh = MeshData::default();

    for &cy in &[RAIL_OFFSET, -RAIL_OFFSET] {
        for &cz in &[RAIL_OFFSET, -RAIL_OFFSET] {
            push_bar(
                &mut mesh,
                Vec3::new(-0.5, cy, cz),
                Vec3::new(0.5, cy, cz),
                RAIL_HALF,
            );
        }
    }

    if style != TrussStyle::NoSupports {
        for i in 0..segments {
            let x0 = -0.5 + i as f32 / segments as f32;
            let x1 = -0.5 + (i + 1) as f32 / segments as f32;
            for &(pos_at, neg_at) in &FACES {
                let forward = (pos_at(x0), neg_at(x1));
                let backward = (neg_at(x0), pos_at(x1));
                // AlternatingSupports zigzags: each segment flips which corner
                // its one diagonal starts from. BridgeStyleSupports crosses
                // both diagonals every segment instead.
                let alternating_forward = style == TrussStyle::AlternatingSupports && i % 2 == 0;
                if style == TrussStyle::BridgeStyleSupports || alternating_forward {
                    push_bar(&mut mesh, forward.0, forward.1, RAIL_HALF);
                }
                if style == TrussStyle::BridgeStyleSupports || !alternating_forward {
                    push_bar(&mut mesh, backward.0, backward.1, RAIL_HALF);
                }
            }
        }
    }

    mesh
}

/// Rotates a canonical (long-axis-X) mesh onto `axis` instead.
///
/// Both rotations used here (90 degrees about Z for Y, about Y for Z) have
/// determinant +1, so applying the same map to positions and normals keeps
/// winding and outward-facing normals correct with no extra fix-up.
pub(crate) fn oriented(mesh: MeshData, axis: TrussAxis) -> MeshData {
    match axis {
        TrussAxis::X => mesh,
        TrussAxis::Y => rotate(mesh, |v| Vec3::new(-v.y, v.x, v.z)),
        TrussAxis::Z => rotate(mesh, |v| Vec3::new(-v.z, v.y, v.x)),
    }
}

fn rotate(mut mesh: MeshData, by: impl Fn(Vec3) -> Vec3) -> MeshData {
    for position in &mut mesh.positions {
        *position = by(Vec3::from(*position)).to_array();
    }
    for normal in &mut mesh.normals {
        *normal = by(Vec3::from(*normal)).to_array();
    }
    mesh
}

/// Appends a thin box running from `p0` to `p1`, `half`-thick in Y and Z.
/// Used for both the straight corner rails (which run along X) and the
/// diagonal braces (which do not).
///
/// The Y/Z offsets stay fixed to those axes rather than to whatever is
/// perpendicular to `p1 - p0`: a brace's own direction always has some X
/// component (it is diagonal), and offsetting perpendicular to that would
/// smear part of the thickness onto X — a small amount in this unit mesh, but
/// one that [`oriented`]'s scale-by-`size` later multiplies by the part's
/// whole long-axis length, overshooting well past the bar's own endpoints.
/// Keeping every offset in the cross-section plane rules that out entirely.
fn push_bar(mesh: &mut MeshData, p0: Vec3, p1: Vec3, half: f32) {
    let y = Vec3::Y * half;
    let z = Vec3::Z * half;
    let center = (p0 + p1) * 0.5;

    let ring = |p: Vec3| [p + y + z, p + y - z, p - y - z, p - y + z];
    let [a0, b0, c0, d0] = ring(p0);
    let [a1, b1, c1, d1] = ring(p1);

    mesh.push_flat_quad(center, a0, b0, c0, d0);
    mesh.push_flat_quad(center, d1, c1, b1, a1);
    mesh.push_flat_quad(center, a0, a1, b1, b0);
    mesh.push_flat_quad(center, b0, b1, c1, c0);
    mesh.push_flat_quad(center, c0, c1, d1, d0);
    mesh.push_flat_quad(center, d0, d1, a1, a0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::test_support::assert_well_formed;

    #[test]
    fn every_style_is_well_formed() {
        for style in [
            TrussStyle::AlternatingSupports,
            TrussStyle::BridgeStyleSupports,
            TrussStyle::NoSupports,
        ] {
            assert_well_formed(&truss(20, style));
        }
    }

    #[test]
    fn oriented_meshes_stay_well_formed_on_every_axis() {
        for axis in [TrussAxis::X, TrussAxis::Y, TrussAxis::Z] {
            assert_well_formed(&oriented(truss(5, TrussStyle::AlternatingSupports), axis));
        }
    }

    #[test]
    fn oriented_moves_the_long_axis_onto_the_requested_one() {
        // Every rail/brace offset sits at one of four fixed cross-section
        // coordinates, however many segments there are; the long axis instead
        // gains one more distinct coordinate per segment boundary. That
        // difference, not extent (both reach +-0.5 by construction), is what
        // marks which axis `oriented` actually moved the length onto.
        let distinct_coordinates = |mesh: &MeshData, axis: usize| {
            let mut values: Vec<i32> = mesh
                .positions
                .iter()
                .map(|p| (p[axis] * 1e4).round() as i32)
                .collect();
            values.sort_unstable();
            values.dedup();
            values.len()
        };

        let canonical = truss(8, TrussStyle::AlternatingSupports);
        assert!(distinct_coordinates(&canonical, 0) > 4);
        assert_eq!(distinct_coordinates(&canonical, 1), 4);
        assert_eq!(distinct_coordinates(&canonical, 2), 4);

        let y = oriented(truss(8, TrussStyle::AlternatingSupports), TrussAxis::Y);
        assert!(distinct_coordinates(&y, 1) > 4);
        assert_eq!(distinct_coordinates(&y, 0), 4);
        assert_eq!(distinct_coordinates(&y, 2), 4);

        let z = oriented(truss(8, TrussStyle::AlternatingSupports), TrussAxis::Z);
        assert!(distinct_coordinates(&z, 2) > 4);
        assert_eq!(distinct_coordinates(&z, 0), 4);
        assert_eq!(distinct_coordinates(&z, 1), 4);
    }

    #[test]
    fn more_segments_means_more_geometry() {
        let short = truss(2, TrussStyle::AlternatingSupports);
        let long = truss(20, TrussStyle::AlternatingSupports);
        assert!(long.indices.len() > short.indices.len());
    }

    #[test]
    fn no_supports_has_only_the_four_rails() {
        // Four rails, twelve triangles each (a closed box), regardless of
        // `segments`, since there is no bracing to repeat.
        let mesh = truss(20, TrussStyle::NoSupports);
        assert_eq!(mesh.indices.len() / 3, 4 * 12);
    }

    #[test]
    fn a_twenty_segment_truss_stays_well_under_ten_thousand_triangles() {
        for style in [
            TrussStyle::AlternatingSupports,
            TrussStyle::BridgeStyleSupports,
        ] {
            let triangles = truss(20, style).indices.len() / 3;
            assert!(triangles < 10_000, "{triangles} triangles");
        }
    }

    // No fixture has a TrussPart to screenshot (see AGENTS.md's fixture list),
    // so this dumps the unit mesh alone as an OBJ for manual inspection in any
    // 3D viewer — run with `cargo test -p rbx_viewer -- --ignored truss_obj`.
    #[test]
    #[ignore]
    fn truss_obj_dump() {
        use std::fmt::Write as _;

        let mesh = truss(20, TrussStyle::AlternatingSupports);
        let mut obj = String::new();
        for position in &mesh.positions {
            writeln!(obj, "v {} {} {}", position[0], position[1], position[2]).unwrap();
        }
        for triangle in mesh.indices.as_chunks::<3>().0 {
            // OBJ face indices are 1-based.
            writeln!(
                obj,
                "f {} {} {}",
                triangle[0] + 1,
                triangle[1] + 1,
                triangle[2] + 1
            )
            .unwrap();
        }

        let path = "/tmp/claude-1000/-mnt-data-Documents-Dev/02662147-12e3-4e0e-a3a2-acc15eb8564c/scratchpad/truss_part/truss.obj";
        std::fs::write(path, &obj).unwrap();
        println!(
            "wrote {path}: {} vertices, {} triangles",
            mesh.positions.len(),
            mesh.indices.len() / 3
        );
    }
}
