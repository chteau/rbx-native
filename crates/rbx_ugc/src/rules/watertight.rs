use std::collections::HashMap;

use crate::{Finding, Geometry, Item, Rule, Severity, Value};

const WELD: f32 = 1e-4;

/// Each vertex's welded id: corners at the same spot (a UV or normal seam
/// splits one corner into several vertices) share one.
pub fn weld(positions: &[[f32; 3]]) -> Vec<u32> {
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    positions
        .iter()
        .map(|p| {
            let key = p.map(|c| (c / WELD).round() as i64);
            let next = ids.len() as u32;
            *ids.entry(key).or_insert(next)
        })
        .collect()
}

/// The welded corners of each triangle (see [`weld`]), skipping
/// out-of-range and collapsed ones.
pub fn welded_triangles<'a>(
    geometry: &'a Geometry,
    welded: &'a [u32],
) -> impl Iterator<Item = [u32; 3]> + 'a {
    geometry.triangles.iter().filter_map(|tri| {
        let corners = tri.map(|i| welded.get(i as usize).copied());
        let [Some(a), Some(b), Some(c)] = corners else {
            return None;
        };
        (a != b && b != c && a != c).then_some([a, b, c])
    })
}

/// The edges not shared by exactly two triangles, holes and non-manifold
/// seams alike, as welded id pairs (see [`weld`]), lower id first.
pub fn open_edges(geometry: &Geometry) -> Vec<[u32; 2]> {
    let welded = weld(&geometry.positions);
    let mut edges: HashMap<[u32; 2], usize> = HashMap::new();
    for [a, b, c] in welded_triangles(geometry, &welded) {
        for (x, y) in [(a, b), (b, c), (c, a)] {
            *edges.entry([x.min(y), x.max(y)]).or_insert(0) += 1;
        }
    }
    let mut open: Vec<[u32; 2]> = edges
        .into_iter()
        .filter(|&(_, n)| n != 2)
        .map(|(e, _)| e)
        .collect();
    open.sort_unstable();
    open
}

pub(crate) fn watertight(item: &Item, out: &mut Vec<Finding>) {
    for part in &item.parts {
        let Some(geometry) = &part.geometry else {
            continue;
        };
        let bad = open_edges(geometry).len();
        if bad > 0 {
            out.push(Finding {
                rule: Rule::Watertight,
                severity: Severity::Warning,
                subject: part.name.clone(),
                measured: Some(Value::Count(bad)),
                limit: Some(Value::Count(0)),
                message: format!(
                    "{bad} open edges: close the holes so the mesh is watertight (Roblox\u{2019}s spec asks for it)."
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetType, BodyScale, Geometry, Part, IDENTITY};

    const CUBE: [[u32; 3]; 12] = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];

    fn cube_positions() -> Vec<[f32; 3]> {
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ]
    }

    fn run(positions: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>) -> Vec<Finding> {
        let item = Item {
            name: "Cube".into(),
            asset_type: AssetType::Hat,
            scale: BodyScale::Normal,
            parts: vec![Part {
                name: "Handle".into(),
                geometry: Some(Geometry {
                    positions,
                    triangles,
                    ..Default::default()
                }),
                cframe: IDENTITY,
                attachments: vec![],
            }],
            textures: vec![],
            cages: vec![],
        };
        let mut out = Vec::new();
        watertight(&item, &mut out);
        out
    }

    #[test]
    fn a_closed_cube_passes() {
        assert!(run(cube_positions(), CUBE.to_vec()).is_empty());
    }

    #[test]
    fn a_cube_missing_a_triangle_has_three_open_edges() {
        let mut tris = CUBE.to_vec();
        tris.pop();
        let out = run(cube_positions(), tris);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].measured, Some(Value::Count(3)));
    }

    #[test]
    fn open_edges_name_the_hole_by_welded_corners() {
        let mut tris = CUBE.to_vec();
        tris.pop();
        let geometry = Geometry {
            positions: cube_positions(),
            triangles: tris,
            ..Default::default()
        };
        assert_eq!(open_edges(&geometry), vec![[3, 4], [3, 7], [4, 7]]);
    }

    #[test]
    fn a_fin_on_an_edge_makes_it_non_manifold() {
        let mut positions = cube_positions();
        positions.push([0.5, 2.0, 0.0]);
        let mut tris = CUBE.to_vec();
        tris.push([2, 3, 8]);
        let geometry = Geometry {
            positions,
            triangles: tris,
            ..Default::default()
        };
        assert_eq!(open_edges(&geometry), vec![[2, 3], [2, 8], [3, 8]]);
    }

    #[test]
    fn a_cube_with_split_vertices_passes_after_welding() {
        let base = cube_positions();
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        for tri in CUBE {
            let start = positions.len() as u32;
            positions.extend(tri.iter().map(|&i| base[i as usize]));
            triangles.push([start, start + 1, start + 2]);
        }
        assert!(run(positions, triangles).is_empty());
    }
}
