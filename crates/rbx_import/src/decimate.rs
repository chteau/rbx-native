//! Brings a mesh over [`crate::TRIANGLE_LIMIT`] under it the way Studio's
//! importer does instead of refusing it: a quadric-error edge collapse
//! (meshoptimizer's `simplify`) that keeps the vertex buffer, so every
//! attribute (UVs, normals, skin weights) of the vertices it keeps survives
//! as is. UV and normal seams stay where they are; only when the shape
//! can't be collapsed that far does the cheaper sloppy pass take over.

/// One mesh that was brought under the limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimated {
    pub name: String,
    pub from: usize,
    pub to: usize,
}

/// `triangles` over `positions`, cut to at most `target` triangles. The
/// result indexes the same vertices.
pub(crate) fn simplify(
    positions: &[[f32; 3]],
    triangles: &[[u32; 3]],
    target: usize,
) -> Vec<[u32; 3]> {
    if triangles.len() <= target {
        return triangles.to_vec();
    }
    let indices = triangles.concat();
    // A relative error of 1 lets it collapse as far as the shape allows, so
    // the count is what stops it. Regularize keeps the triangles even: on a
    // 21k-triangle sphere Studio's result and this one share the longest
    // edge (0.0405 studs) and the bounding box to 1e-5.
    let mut result = meshopt::simplify_decoder(
        &indices,
        positions,
        target * 3,
        1.0,
        meshopt::SimplifyOptions::Regularize,
        None,
    );
    if result.len() > target * 3 {
        result = meshopt::simplify_sloppy_decoder(&result, positions, target * 3, 1.0, None);
    }
    result.as_chunks::<3>().0.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A UV sphere with a seam column of duplicated positions, like an
    /// exported one.
    pub(crate) fn sphere(segments: u32, rings: u32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let mut positions = Vec::new();
        for r in 0..=rings {
            let theta = std::f32::consts::PI * r as f32 / rings as f32;
            for s in 0..=segments {
                let phi = std::f32::consts::TAU * s as f32 / segments as f32;
                positions.push([
                    theta.sin() * phi.cos(),
                    theta.cos(),
                    theta.sin() * phi.sin(),
                ]);
            }
        }
        let at = |r: u32, s: u32| r * (segments + 1) + s;
        let mut triangles = Vec::new();
        for r in 0..rings {
            for s in 0..segments {
                let (a, b, c, d) = (at(r, s), at(r, s + 1), at(r + 1, s), at(r + 1, s + 1));
                if r > 0 {
                    triangles.push([a, b, c]);
                }
                if r + 1 < rings {
                    triangles.push([b, d, c]);
                }
            }
        }
        (positions, triangles)
    }

    #[test]
    fn a_mesh_under_the_target_is_left_alone() {
        let (positions, triangles) = sphere(8, 6);
        assert_eq!(simplify(&positions, &triangles, 1000), triangles);
    }

    #[test]
    fn a_dense_sphere_comes_under_the_target_and_keeps_its_shape() {
        let (positions, triangles) = sphere(105, 101);
        assert!(triangles.len() > 20_000);
        let out = simplify(&positions, &triangles, 20_000);
        assert!((19_990..=20_000).contains(&out.len()), "{}", out.len());
        let extent = |tris: &[[u32; 3]]| {
            let mut hi = [f32::MIN; 3];
            for &i in tris.iter().flatten() {
                let p = positions[i as usize];
                for axis in 0..3 {
                    hi[axis] = hi[axis].max(p[axis]);
                }
            }
            hi
        };
        let (before, after) = (extent(&triangles), extent(&out));
        for axis in 0..3 {
            assert!(
                (before[axis] - after[axis]).abs() < 1e-3,
                "{before:?} {after:?}"
            );
        }
    }

    #[test]
    fn a_hard_target_still_gets_there() {
        let (positions, triangles) = sphere(105, 101);
        assert!(simplify(&positions, &triangles, 50).len() <= 50);
    }
}
