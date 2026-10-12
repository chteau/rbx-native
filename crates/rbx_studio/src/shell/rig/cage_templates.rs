//! The stock Mannequin's outer cages, downloaded once and kept for the
//! session: the templates the auto-rigger bends onto a character's fit.

use std::sync::{Arc, Mutex};

use glam::{Vec2, Vec3};
use rbx_autorig::{CageTemplate, Mesh, Part};

use super::bundle::{stock, Family, Piece};
use super::BodyShape;

static MEMO: Mutex<Option<Arc<Vec<CageTemplate>>>> = Mutex::new(None);

/// The cage template of every Mannequin body part. Blocking: fetches what
/// the asset cache lacks, so call it off the UI thread. Only a success is
/// remembered.
pub(super) fn mannequin() -> Result<Arc<Vec<CageTemplate>>, String> {
    let mut memo = MEMO.lock().map_err(|e| e.to_string())?;
    if let Some(done) = memo.as_ref() {
        return Ok(done.clone());
    }
    let cache = rbx_assets::AssetCache::new(None).map_err(|e| e.to_string())?;
    let client = rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config());
    let mut templates = Vec::new();
    for piece in stock(Family::Mannequin, BodyShape::Masculine).values() {
        let Some((id, _)) = piece.cage else {
            continue;
        };
        let Some(part) = Part::ALL.into_iter().find(|p| p.name() == piece.name) else {
            continue;
        };
        let bytes = match cache.get_id(id) {
            Some(bytes) => bytes,
            None => {
                let bytes = client
                    .asset(id)
                    .map_err(|e| format!("downloading the {} cage: {e}", piece.name))?
                    .bytes;
                let _ = cache.put_id(id, &bytes);
                bytes
            }
        };
        let mesh = rbx_mesh::parse(&bytes)
            .map_err(|e| format!("reading the {} cage: {e}", piece.name))?;
        templates.push(template(part, piece, &mesh));
    }
    let done = Arc::new(templates);
    *memo = Some(done.clone());
    Ok(done)
}

/// `piece`'s cage `mesh` as a template, with the cage origin added to every
/// position so the cages of neighbouring parts meet at their seams.
fn template(part: Part, piece: &Piece, mesh: &rbx_mesh::Mesh) -> CageTemplate {
    let origin = piece.cage.map_or(Vec3::ZERO, |(_, o)| Vec3::from(o));
    CageTemplate {
        part,
        mesh: Mesh {
            positions: mesh
                .vertices
                .iter()
                .map(|v| Vec3::from(v.position) + origin)
                .collect(),
            normals: mesh.vertices.iter().map(|v| Vec3::from(v.normal)).collect(),
            uvs: mesh.vertices.iter().map(|v| Vec2::from(v.uv)).collect(),
            triangles: mesh
                .indices
                .as_chunks::<3>()
                .0
                .to_vec(),
            ..Mesh::default()
        },
        size: Vec3::from(piece.init),
        attachments: piece
            .rig
            .iter()
            .map(|(n, p)| (n.clone(), Vec3::from(*p)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::rig::bundle::piece;

    #[test]
    fn template_adds_the_origin_and_copies_uvs_and_triangles() {
        let mut stock = piece(Family::Mannequin, BodyShape::Masculine, "Head").unwrap();
        stock.cage = Some((1, [1.0, 2.0, 3.0]));
        let vertex = |x: f32| rbx_mesh::Vertex {
            position: [x, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            uv: [x, 0.5],
            color: [255; 4],
        };
        let mesh = rbx_mesh::Mesh {
            version: (2, 0),
            vertices: vec![vertex(0.0), vertex(1.0), vertex(2.0)],
            indices: vec![0, 1, 2],
            lods: Vec::new(),
            bounds: rbx_mesh::Aabb {
                min: [0.0; 3],
                max: [2.0, 0.0, 0.0],
            },
        };
        let t = template(Part::Head, &stock, &mesh);
        assert_eq!(t.mesh.positions[1], Vec3::new(2.0, 2.0, 3.0));
        assert_eq!(t.mesh.uvs[2], Vec2::new(2.0, 0.5));
        assert_eq!(t.mesh.triangles, vec![[0, 1, 2]]);
        assert_eq!(t.size, Vec3::from(stock.init));
    }
}
