use super::distance::{cross, length};
use crate::{Finding, Geometry, Item, Rule, Severity, Value};

/// Below this, a triangle's area over its longest edge squared counts as
/// no area at all. Studio 0.748 counted 17 of mesh 2938588047's 1004
/// triangles (1.70%): the ones under 1e-6, the next being 1.18e-6.
const AREA_RATIO: f32 = 1e-6;

/// The triangles with zero or near-zero area, by index in
/// `geometry.triangles`.
pub fn degenerate_triangles(geometry: &Geometry) -> Vec<usize> {
    geometry
        .triangles
        .iter()
        .enumerate()
        .filter(|(_, tri)| {
            let corners = tri.map(|i| geometry.positions.get(i as usize).copied());
            let [Some(a), Some(b), Some(c)] = corners else {
                return false;
            };
            let edge = |p: [f32; 3], q: [f32; 3]| [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
            let (ab, ac, bc) = (edge(a, b), edge(a, c), edge(b, c));
            let longest = length(ab).max(length(ac)).max(length(bc));
            let area = 0.5 * length(cross(ab, ac));
            longest == 0.0 || area <= AREA_RATIO * longest * longest
        })
        .map(|(i, _)| i)
        .collect()
}

pub(crate) fn degenerate(item: &Item, out: &mut Vec<Finding>) {
    for part in &item.parts {
        let Some(geometry) = &part.geometry else {
            continue;
        };
        let bad = degenerate_triangles(geometry).len();
        if bad == 0 {
            continue;
        }
        let percent = 100.0 * bad as f32 / geometry.triangles.len() as f32;
        out.push(Finding {
            rule: Rule::DegenerateTriangles,
            severity: Severity::Warning,
            subject: part.name.clone(),
            measured: Some(Value::Count(bad)),
            limit: Some(Value::Count(0)),
            message: format!(
                "{} has {} degenerate triangles.",
                part.name,
                super::percent(percent)
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetType, BodyScale, Part, IDENTITY};

    fn geometry(positions: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>) -> Geometry {
        Geometry {
            positions,
            triangles,
            ..Default::default()
        }
    }

    fn quad_with(extra: [f32; 3]) -> Geometry {
        geometry(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                extra,
            ],
            vec![[0, 1, 2], [0, 2, 3], [0, 1, 4]],
        )
    }

    #[test]
    fn a_proper_triangle_is_not_degenerate() {
        assert!(degenerate_triangles(&quad_with([0.5, -1.0, 0.0])).is_empty());
    }

    #[test]
    fn a_triangle_with_a_point_on_its_edge_is_degenerate() {
        assert_eq!(degenerate_triangles(&quad_with([0.5, 0.0, 0.0])), vec![2]);
    }

    #[test]
    fn a_triangle_with_two_corners_at_one_spot_is_degenerate() {
        assert_eq!(degenerate_triangles(&quad_with([1.0, 0.0, 0.0])), vec![2]);
        let g = geometry(vec![[0.0; 3]], vec![[0, 0, 0]]);
        assert_eq!(degenerate_triangles(&g), vec![0]);
    }

    #[test]
    fn a_sliver_counts_but_a_small_triangle_does_not() {
        assert_eq!(degenerate_triangles(&quad_with([0.5, 1e-7, 0.0])), vec![2]);
        let tiny = geometry(
            vec![[0.0; 3], [1e-3, 0.0, 0.0], [0.0, 1e-3, 0.0]],
            vec![[0, 1, 2]],
        );
        assert!(degenerate_triangles(&tiny).is_empty());
    }

    #[test]
    fn the_finding_names_the_part_and_the_share() {
        let item = Item {
            name: "Hat".into(),
            asset_type: AssetType::Hat,
            scale: BodyScale::Normal,
            parts: vec![Part {
                name: "Handle".into(),
                geometry: Some(quad_with([0.5, 0.0, 0.0])),
                cframe: IDENTITY,
                attachments: vec![],
            }],
            textures: vec![],
            cages: vec![],
        };
        let mut out = Vec::new();
        degenerate(&item, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rule, Rule::DegenerateTriangles);
        assert_eq!(out[0].severity, Severity::Warning);
        assert_eq!(out[0].measured, Some(Value::Count(1)));
        assert_eq!(out[0].message, "Handle has 33.30% degenerate triangles.");
    }
}
