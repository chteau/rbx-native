//! Binary `.fbx`: a polygon index array ends each polygon on a negative
//! entry, and a polygon of `n` corners triangulates to `n - 2` triangles.

use fbxcel::tree::any::AnyTree;
use fbxcel::tree::v7400::NodeHandle;

use crate::{MeshInfo, Report};

pub(crate) fn report(bytes: &[u8]) -> Report {
    let tree = match AnyTree::from_seekable_reader(std::io::Cursor::new(bytes)) {
        Ok(AnyTree::V7400(_, tree, _)) => tree,
        Ok(_) => return unchecked("this FBX version"),
        Err(_) => return unchecked("an ASCII or unreadable FBX"),
    };
    let mut meshes = Vec::new();
    for objects in tree.root().children_by_name("Objects") {
        for geometry in objects.children_by_name("Geometry") {
            if let Some(triangles) = triangles(geometry) {
                meshes.push(MeshInfo {
                    name: name_of(geometry),
                    triangles,
                });
            }
        }
    }
    Report {
        meshes,
        unchecked: None,
    }
}

fn unchecked(what: &str) -> Report {
    Report {
        meshes: Vec::new(),
        unchecked: Some(format!(
            "Couldn\u{2019}t count the triangles of {what}; Roblox will check the {}-triangle limit on upload",
            crate::TRIANGLE_LIMIT
        )),
    }
}

fn triangles(geometry: NodeHandle<'_>) -> Option<usize> {
    let indices = geometry
        .children_by_name("PolygonVertexIndex")
        .next()?
        .attributes()
        .first()?
        .get_arr_i32()?;
    let mut corners = 0usize;
    let mut total = 0;
    for &index in indices {
        corners += 1;
        if index < 0 {
            total += corners.saturating_sub(2);
            corners = 0;
        }
    }
    Some(total)
}

/// FBX names read `Name\0\x01Geometry`.
fn name_of(geometry: NodeHandle<'_>) -> String {
    let name = geometry
        .attributes()
        .get(1)
        .and_then(|a| a.get_string())
        .and_then(|s| s.split('\0').next())
        .filter(|s| !s.is_empty());
    name.map_or_else(|| "Geometry".to_string(), str::to_owned)
}
