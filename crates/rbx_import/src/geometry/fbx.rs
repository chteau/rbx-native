//! Binary FBX geometry: control points, fan-triangulated polygons, the first
//! UV seen per control point, and bone counts from skin clusters.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;

use fbxcel::tree::any::AnyTree;
use fbxcel::tree::v7400::NodeHandle;

use super::{add_side_texture, SourceGeometry, SourceMesh};

pub(super) fn read(bytes: &[u8], path: &Path) -> Result<SourceGeometry, String> {
    let mut out = SourceGeometry::default();
    let tree = match AnyTree::from_seekable_reader(Cursor::new(bytes)) {
        Ok(AnyTree::V7400(_, tree, _)) => tree,
        Ok(_) => {
            out.notes.push(
                "This FBX version can\u{2019}t be read here; export FBX 7.4 binary to check it."
                    .to_string(),
            );
            return Ok(out);
        }
        Err(err) if bytes.starts_with(b"Kaydara FBX Binary") => {
            return Err(format!("Not a valid FBX file: {err}"));
        }
        Err(_) => {
            out.notes.push(
                "ASCII FBX files can\u{2019}t be read here; export binary FBX to check them."
                    .to_string(),
            );
            return Ok(out);
        }
    };
    let dir = path.parent().unwrap_or(Path::new(""));
    let root = tree.root();

    let links: Vec<(i64, i64)> = root
        .children_by_name("Connections")
        .flat_map(|c| c.children_by_name("C"))
        .filter_map(|c| {
            let a = c.attributes();
            if a.first()?.get_string()? != "OO" {
                return None;
            }
            Some((a.get(1)?.get_i64()?, a.get(2)?.get_i64()?))
        })
        .collect();

    let mut models: HashMap<i64, String> = HashMap::new();
    let mut skins: HashSet<i64> = HashSet::new();
    let mut clusters: Vec<(i64, NodeHandle<'_>)> = Vec::new();
    let mut geometries: Vec<(i64, NodeHandle<'_>)> = Vec::new();
    for node in root.children_by_name("Objects").flat_map(|o| o.children()) {
        let id = node.attributes().first().and_then(|a| a.get_i64());
        match (node.name(), id) {
            ("Model", Some(id)) => {
                models.insert(id, name_of(node).unwrap_or_default());
            }
            ("Geometry", Some(id)) => geometries.push((id, node)),
            ("Deformer", Some(id)) => match node.attributes().get(2).and_then(|a| a.get_string()) {
                Some("Skin") => {
                    skins.insert(id);
                }
                Some("Cluster") => clusters.push((id, node)),
                _ => {}
            },
            ("Texture", _) => {
                for key in ["RelativeFilename", "FileName"] {
                    let reference = node
                        .children_by_name(key)
                        .next()
                        .and_then(|n| n.attributes().first()?.get_string())
                        .filter(|s| !s.is_empty());
                    if let Some(reference) = reference {
                        add_side_texture(dir, reference, &mut out);
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    for (id, geometry) in geometries {
        let Some(mut mesh) = mesh_of(geometry) else {
            continue;
        };
        let model = links
            .iter()
            .find_map(|&(child, parent)| (child == id).then(|| models.get(&parent)).flatten());
        mesh.name = model
            .filter(|n| !n.is_empty())
            .cloned()
            .or_else(|| name_of(geometry))
            .unwrap_or_else(|| "Geometry".to_string());

        let own_skins: Vec<i64> = links
            .iter()
            .filter(|&&(child, parent)| parent == id && skins.contains(&child))
            .map(|&(child, _)| child)
            .collect();
        let own_clusters: Vec<(i64, NodeHandle<'_>)> = clusters
            .iter()
            .filter(|(cluster, _)| {
                links
                    .iter()
                    .any(|&(child, parent)| child == *cluster && own_skins.contains(&parent))
            })
            .copied()
            .collect();
        if !own_clusters.is_empty() {
            let nodes: Vec<NodeHandle<'_>> = own_clusters.iter().map(|&(_, node)| node).collect();
            mesh.influences = influences(mesh.positions.len(), &nodes);
            // Each cluster is linked to the bone it binds.
            mesh.bones = own_clusters
                .iter()
                .filter_map(|&(cluster, _)| {
                    links
                        .iter()
                        .find_map(|&(child, parent)| {
                            (parent == cluster).then(|| models.get(&child)).flatten()
                        })
                        .cloned()
                })
                .collect();
        }
        out.meshes.push(mesh);
    }
    Ok(out)
}

/// FBX names read `Name\0\x01Class`.
fn name_of(node: NodeHandle<'_>) -> Option<String> {
    node.attributes()
        .get(1)?
        .get_string()?
        .split('\0')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn floats(node: NodeHandle<'_>, name: &str) -> Option<Vec<f64>> {
    let value = node.children_by_name(name).next()?.attributes().first()?;
    value.get_arr_f64().map(<[f64]>::to_vec).or_else(|| {
        value
            .get_arr_f32()
            .map(|v| v.iter().map(|&x| f64::from(x)).collect())
    })
}

fn ints(node: NodeHandle<'_>, name: &str) -> Option<Vec<i32>> {
    let value = node.children_by_name(name).next()?.attributes().first()?;
    value.get_arr_i32().map(<[i32]>::to_vec)
}

/// A polygon's last index is stored as `!i`.
fn control_point(raw: i32) -> usize {
    (if raw < 0 { !raw } else { raw }) as usize
}

fn mesh_of(geometry: NodeHandle<'_>) -> Option<SourceMesh> {
    let vertices = floats(geometry, "Vertices")?;
    let indices = ints(geometry, "PolygonVertexIndex")?;
    let positions: Vec<[f32; 3]> = vertices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|v| [v[0] as f32, v[1] as f32, v[2] as f32])
        .collect();

    let mut triangles = Vec::new();
    let mut polygon: Vec<u32> = Vec::new();
    for &raw in &indices {
        polygon.push(control_point(raw) as u32);
        if raw < 0 {
            for k in 1..polygon.len().saturating_sub(1) {
                triangles.push([polygon[0], polygon[k], polygon[k + 1]]);
            }
            polygon.clear();
        }
    }
    triangles.retain(|t| t.iter().all(|&i| (i as usize) < positions.len()));

    Some(SourceMesh {
        name: String::new(),
        uvs: uvs(geometry, &indices, positions.len()),
        positions,
        triangles,
        ..Default::default()
    })
}

/// UVs belong to polygon corners; a vertex shared by several polygons keeps
/// the first one seen.
fn uvs(geometry: NodeHandle<'_>, indices: &[i32], count: usize) -> Vec<[f32; 2]> {
    let Some(layer) = geometry.children_by_name("LayerElementUV").next() else {
        return Vec::new();
    };
    let Some(data) = floats(layer, "UV").filter(|d| !d.is_empty()) else {
        return Vec::new();
    };
    let mapping = layer
        .children_by_name("MappingInformationType")
        .next()
        .and_then(|n| n.attributes().first()?.get_string());
    let by_point = matches!(mapping, Some("ByControlPoint" | "ByVertice" | "ByVertex"));
    let uv_index = ints(layer, "UVIndex");

    let mut found: Vec<Option<[f32; 2]>> = vec![None; count];
    for (corner, &raw) in indices.iter().enumerate() {
        let point = control_point(raw);
        let Some(slot) = found.get_mut(point) else {
            continue;
        };
        if slot.is_some() {
            continue;
        }
        let at = if by_point { point } else { corner };
        let at = match &uv_index {
            Some(map) => map.get(at).and_then(|&i| usize::try_from(i).ok()),
            None => Some(at),
        };
        if let Some(uv) = at.and_then(|i| data.get(i * 2..i * 2 + 2)) {
            *slot = Some([uv[0] as f32, uv[1] as f32]);
        }
    }
    found.into_iter().map(Option::unwrap_or_default).collect()
}

fn influences(count: usize, clusters: &[NodeHandle<'_>]) -> Vec<u8> {
    let mut bones = vec![0u8; count];
    for &cluster in clusters {
        let (Some(indexes), Some(weights)) = (ints(cluster, "Indexes"), floats(cluster, "Weights"))
        else {
            continue;
        };
        for (&index, &weight) in indexes.iter().zip(&weights) {
            if weight == 0.0 {
                continue;
            }
            if let Some(slot) = usize::try_from(index).ok().and_then(|i| bones.get_mut(i)) {
                *slot = slot.saturating_add(1);
            }
        }
    }
    bones
}
