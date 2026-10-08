//! Binary and ASCII `.fbx`: a polygon index array ends each polygon on a
//! negative entry, and a polygon of `n` corners triangulates to `n - 2`
//! triangles.

use fbxcel::tree::any::AnyTree;
use fbxcel::tree::v7400::NodeHandle;

use crate::{MeshInfo, Report};

pub(crate) fn report(bytes: &[u8]) -> Report {
    let tree = match AnyTree::from_seekable_reader(std::io::Cursor::new(bytes)) {
        Ok(AnyTree::V7400(_, tree, _)) => tree,
        Ok(_) => return unchecked("this FBX version"),
        Err(_) => return ascii(bytes).unwrap_or_else(|| unchecked("an unreadable FBX")),
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
    Some(count(indices.iter().copied()))
}

fn count(indices: impl Iterator<Item = i32>) -> usize {
    let mut corners = 0usize;
    let mut total = 0;
    for index in indices {
        corners += 1;
        if index < 0 {
            total += corners.saturating_sub(2);
            corners = 0;
        }
    }
    total
}

/// The text form (`Geometry: id, "Geometry::Name", "Mesh" {` … `PolygonVertexIndex: *n { a: 0,1,-3 }`;
/// FBX 6.1 writes the numbers inline). `None` when the file is not text.
fn ascii(bytes: &[u8]) -> Option<Report> {
    let text = std::str::from_utf8(bytes).ok()?;
    if !text.contains("FBXHeaderExtension") {
        return None;
    }
    let mut meshes = Vec::new();
    let mut geometries = text
        .lines()
        .scan(0, |at, line| {
            let start = *at;
            *at += line.len() + 1;
            Some((start, line.trim_start()))
        })
        .filter(|(_, line)| line.starts_with("Geometry:"))
        .map(|(start, _)| start)
        .peekable();
    while let Some(start) = geometries.next() {
        let end = geometries.peek().copied().unwrap_or(text.len());
        let block = &text[start..end.max(start)];
        let Some(key) = block.find("PolygonVertexIndex:") else {
            continue;
        };
        let rest = block[key + "PolygonVertexIndex:".len()..].trim_start();
        let rest = match rest.strip_prefix('*') {
            Some(sized) => sized.split_once("a:").map_or("", |(_, numbers)| numbers),
            None => rest,
        };
        let numbers: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || c.is_whitespace() || matches!(c, '-' | ','))
            .collect();
        let triangles = count(
            numbers
                .split(',')
                .filter_map(|n| n.trim().parse::<i32>().ok()),
        );
        meshes.push(MeshInfo {
            name: ascii_name(block),
            triangles,
        });
    }
    Some(Report {
        meshes,
        unchecked: None,
    })
}

/// `Geometry: 1, "Geometry::Cube", "Mesh"` names the mesh `Cube`.
fn ascii_name(block: &str) -> String {
    block
        .lines()
        .next()
        .and_then(|line| line.split('"').nth(1))
        .map(|n| n.rsplit("::").next().unwrap_or(n))
        .filter(|n| !n.is_empty())
        .map_or_else(|| "Geometry".to_string(), str::to_owned)
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
