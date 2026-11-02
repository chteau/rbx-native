//! Binary FBX with its over-budget geometries decimated. The file is written
//! out again node by node, so everything but the rewritten geometries
//! (skin clusters, materials, animation) survives as is.

use std::collections::HashMap;
use std::io::{Cursor, Seek, Write};

use fbxcel::low::v7400::AttributeValue;
use fbxcel::tree::any::AnyTree;
use fbxcel::tree::v7400::NodeHandle;
use fbxcel::writer::v7400::binary::{FbxFooter, Writer};

use crate::Decimated;

/// How one child node differs in the rewritten file.
enum Edit {
    Drop,
    Replace(AttributeValue),
    Nested(HashMap<usize, Edit>),
}

/// The array children each layer element type owns: the direct arrays with
/// their tuple widths, and the index array if it has one.
type LayerSpec = (&'static [(&'static str, usize)], Option<&'static str>);

/// The rewritten file and each geometry that was decimated.
type Rewritten = (Vec<u8>, Vec<Decimated>);
/// A geometry's edits by node, with its triangle counts before and after.
type GeometryEdits = (HashMap<usize, Edit>, usize, usize);

/// `Ok(None)` when `bytes` is not a binary FBX 7.x file.
pub(crate) fn decimate(bytes: &[u8], limit: usize) -> Result<Option<Rewritten>, String> {
    let (version, tree) = match AnyTree::from_seekable_reader(Cursor::new(bytes)) {
        Ok(AnyTree::V7400(version, tree, _)) => (version, tree),
        _ => return Ok(None),
    };
    let root = tree.root();
    let mut decimated = Vec::new();
    let mut root_edits: HashMap<usize, Edit> = HashMap::new();
    for (i, objects) in root.children().enumerate() {
        if objects.name() != "Objects" {
            continue;
        }
        let mut object_edits = HashMap::new();
        for (j, geometry) in objects.children().enumerate() {
            if geometry.name() != "Geometry" {
                continue;
            }
            if let Some((edits, from, to)) = plan(geometry, limit)? {
                object_edits.insert(j, Edit::Nested(edits));
                decimated.push(Decimated {
                    name: name_of(geometry),
                    from,
                    to,
                });
            }
        }
        root_edits.insert(i, Edit::Nested(object_edits));
    }

    let mut out = Vec::new();
    {
        let mut writer =
            Writer::new(Cursor::new(&mut out), version).map_err(|e| format!("FBX write: {e}"))?;
        for (i, child) in root.children().enumerate() {
            write_node(&mut writer, child, root_edits.get(&i))?;
        }
        writer
            .finalize_and_flush(&FbxFooter::default())
            .map_err(|e| format!("FBX write: {e}"))?;
    }
    Ok(Some((out, decimated)))
}

fn write_node<W: Write + Seek>(
    w: &mut Writer<W>,
    node: NodeHandle<'_>,
    edit: Option<&Edit>,
) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("FBX write: {e}");
    let attributes: &[AttributeValue] = match edit {
        Some(Edit::Drop) => return Ok(()),
        Some(Edit::Replace(value)) => std::slice::from_ref(value),
        _ => node.attributes(),
    };
    {
        let mut a = w.new_node(node.name()).map_err(|e| fail(&e))?;
        for value in attributes {
            match value {
                AttributeValue::Bool(v) => a.append_bool(*v),
                AttributeValue::I16(v) => a.append_i16(*v),
                AttributeValue::I32(v) => a.append_i32(*v),
                AttributeValue::I64(v) => a.append_i64(*v),
                AttributeValue::F32(v) => a.append_f32(*v),
                AttributeValue::F64(v) => a.append_f64(*v),
                AttributeValue::ArrBool(v) => a.append_arr_bool_from_iter(None, v.iter().copied()),
                AttributeValue::ArrI32(v) => a.append_arr_i32_from_iter(None, v.iter().copied()),
                AttributeValue::ArrI64(v) => a.append_arr_i64_from_iter(None, v.iter().copied()),
                AttributeValue::ArrF32(v) => a.append_arr_f32_from_iter(None, v.iter().copied()),
                AttributeValue::ArrF64(v) => a.append_arr_f64_from_iter(None, v.iter().copied()),
                AttributeValue::String(v) => a.append_string_direct(v),
                AttributeValue::Binary(v) => a.append_binary_direct(v),
            }
            .map_err(|e| fail(&e))?;
        }
    }
    let nested = match edit {
        Some(Edit::Nested(map)) => Some(map),
        _ => None,
    };
    if !matches!(edit, Some(Edit::Replace(_))) {
        for (i, child) in node.children().enumerate() {
            write_node(w, child, nested.and_then(|m| m.get(&i)))?;
        }
    }
    w.close_node().map_err(|e| fail(&e))
}

fn spec(layer: &str) -> Option<LayerSpec> {
    Some(match layer {
        "LayerElementNormal" => (&[("Normals", 3), ("NormalsW", 1)], Some("NormalsIndex")),
        "LayerElementBinormal" => (
            &[("Binormals", 3), ("BinormalsW", 1)],
            Some("BinormalsIndex"),
        ),
        "LayerElementTangent" => (&[("Tangents", 3), ("TangentsW", 1)], Some("TangentsIndex")),
        "LayerElementUV" => (&[("UV", 2)], Some("UVIndex")),
        "LayerElementColor" => (&[("Colors", 4)], Some("ColorIndex")),
        "LayerElementMaterial" => (&[("Materials", 1)], None),
        "LayerElementSmoothing" => (&[("Smoothing", 1)], None),
        "LayerElementVisibility" => (&[("Visibility", 1)], None),
        _ => return None,
    })
}

fn child_value(node: NodeHandle<'_>, name: &str) -> Option<(usize, AttributeValue)> {
    node.children()
        .enumerate()
        .find(|(_, c)| c.name() == name)
        .and_then(|(i, c)| Some((i, c.attributes().first()?.clone())))
}

fn child_string(node: NodeHandle<'_>, name: &str) -> Option<String> {
    node.children_by_name(name)
        .next()?
        .attributes()
        .first()?
        .get_string()
        .map(str::to_owned)
}

fn pick<T: Clone>(values: &[T], source: &[usize], width: usize) -> Result<Vec<T>, String> {
    let mut out = Vec::with_capacity(source.len() * width);
    for &s in source {
        let tuple = values
            .get(s * width..s * width + width)
            .ok_or_else(|| "An FBX layer element is shorter than its geometry".to_string())?;
        out.extend_from_slice(tuple);
    }
    Ok(out)
}

fn gather(
    value: &AttributeValue,
    source: &[usize],
    width: usize,
) -> Result<AttributeValue, String> {
    Ok(match value {
        AttributeValue::ArrI32(v) => AttributeValue::ArrI32(pick(v, source, width)?),
        AttributeValue::ArrI64(v) => AttributeValue::ArrI64(pick(v, source, width)?),
        AttributeValue::ArrF32(v) => AttributeValue::ArrF32(pick(v, source, width)?),
        AttributeValue::ArrF64(v) => AttributeValue::ArrF64(pick(v, source, width)?),
        AttributeValue::ArrBool(v) => AttributeValue::ArrBool(pick(v, source, width)?),
        _ => return Err("An FBX layer element holds something other than an array".to_string()),
    })
}

/// The edits for one layer element so its entries follow `source` (one source
/// entry per new corner or polygon).
fn layer_edits(
    layer: NodeHandle<'_>,
    spec: LayerSpec,
    source: &[usize],
) -> Result<HashMap<usize, Edit>, String> {
    let (datas, index) = spec;
    let indexed =
        child_string(layer, "ReferenceInformationType").as_deref() == Some("IndexToDirect");
    let mut edits = HashMap::new();
    if let Some((at, value)) = index
        .filter(|_| indexed)
        .and_then(|name| child_value(layer, name))
    {
        edits.insert(at, Edit::Replace(gather(&value, source, 1)?));
        return Ok(edits);
    }
    for &(name, width) in datas {
        if let Some((at, value)) = child_value(layer, name) {
            edits.insert(at, Edit::Replace(gather(&value, source, width)?));
        }
    }
    Ok(edits)
}

fn plan(geometry: NodeHandle<'_>, limit: usize) -> Result<Option<GeometryEdits>, String> {
    let Some((_, vertices)) = child_value(geometry, "Vertices") else {
        return Ok(None);
    };
    let Some((_, AttributeValue::ArrI32(indices))) = child_value(geometry, "PolygonVertexIndex")
    else {
        return Ok(None);
    };
    let flat: Vec<f32> = match &vertices {
        AttributeValue::ArrF64(v) => v.iter().map(|&x| x as f32).collect(),
        AttributeValue::ArrF32(v) => v.clone(),
        _ => return Ok(None),
    };
    let positions: Vec<[f32; 3]> = flat.as_chunks::<3>().0.to_vec();

    let mut corner_cp = Vec::with_capacity(indices.len());
    let mut corner_polygon = Vec::with_capacity(indices.len());
    let mut triangles: Vec<[usize; 3]> = Vec::new();
    let mut triangle_polygon = Vec::new();
    let (mut start, mut polygon) = (0usize, 0usize);
    for (i, &raw) in indices.iter().enumerate() {
        let cp = (if raw < 0 { !raw } else { raw }) as usize;
        if cp >= positions.len() {
            return Err("An FBX polygon uses a vertex that does not exist".to_string());
        }
        corner_cp.push(cp);
        corner_polygon.push(polygon);
        if raw < 0 {
            for k in start + 1..i {
                triangles.push([start, k, k + 1]);
                triangle_polygon.push(polygon);
            }
            start = i + 1;
            polygon += 1;
        }
    }
    if triangles.len() <= limit {
        return Ok(None);
    }

    let layers: Vec<(usize, NodeHandle<'_>, LayerSpec)> = geometry
        .children()
        .enumerate()
        .filter_map(|(i, c)| Some((i, c, spec(c.name())?)))
        .collect();
    let corner_values: Vec<Vec<i64>> = layers
        .iter()
        .filter(|(_, layer, _)| {
            child_string(*layer, "MappingInformationType").as_deref() == Some("ByPolygonVertex")
        })
        .map(|(_, layer, (_, index))| {
            let indexed = child_string(*layer, "ReferenceInformationType").as_deref()
                == Some("IndexToDirect");
            let map = match index
                .filter(|_| indexed)
                .and_then(|n| child_value(*layer, n))
            {
                Some((_, AttributeValue::ArrI32(map))) => Some(map),
                _ => None,
            };
            (0..indices.len())
                .map(|c| match &map {
                    Some(map) => map.get(c).map_or(c as i64, |&v| i64::from(v)),
                    None => c as i64,
                })
                .collect()
        })
        .collect();

    let mut ids: HashMap<Vec<i64>, u32> = HashMap::new();
    let mut wedge_source: Vec<usize> = Vec::new();
    let mut corner_wedge: Vec<u32> = Vec::with_capacity(indices.len());
    for c in 0..indices.len() {
        let mut key = Vec::with_capacity(1 + corner_values.len());
        key.push(corner_cp[c] as i64);
        key.extend(corner_values.iter().map(|v| v[c]));
        let next = wedge_source.len() as u32;
        let id = *ids.entry(key).or_insert(next);
        if id == next {
            wedge_source.push(c);
        }
        corner_wedge.push(id);
    }
    let wedge_positions: Vec<[f32; 3]> = wedge_source
        .iter()
        .map(|&c| positions[corner_cp[c]])
        .collect();
    let wedge_triangles: Vec<[u32; 3]> = triangles
        .iter()
        .map(|t| t.map(|c| corner_wedge[c]))
        .collect();
    let kept = crate::decimate::simplify(&wedge_positions, &wedge_triangles, limit);

    let sorted = |t: [u32; 3]| {
        let mut t = t;
        t.sort_unstable();
        t
    };
    let mut owner: HashMap<[u32; 3], usize> = HashMap::new();
    for (t, &p) in wedge_triangles.iter().zip(&triangle_polygon) {
        owner.entry(sorted(*t)).or_insert(p);
    }

    let mut new_indices = Vec::with_capacity(kept.len() * 3);
    let mut new_corners = Vec::with_capacity(kept.len() * 3);
    let mut new_polygons = Vec::with_capacity(kept.len());
    for t in &kept {
        for (n, &wedge) in t.iter().enumerate() {
            let source = wedge_source[wedge as usize];
            let cp = corner_cp[source] as i32;
            new_indices.push(if n == 2 { !cp } else { cp });
            new_corners.push(source);
        }
        new_polygons.push(
            owner
                .get(&sorted(*t))
                .copied()
                .unwrap_or(corner_polygon[wedge_source[t[0] as usize]]),
        );
    }

    let mut edits = HashMap::new();
    for (i, child) in geometry.children().enumerate() {
        match child.name() {
            "PolygonVertexIndex" => {
                edits.insert(
                    i,
                    Edit::Replace(AttributeValue::ArrI32(new_indices.clone())),
                );
            }
            "Edges" => {
                edits.insert(i, Edit::Drop);
            }
            _ => {}
        }
    }
    for (i, layer, layer_spec) in layers {
        let source = match child_string(layer, "MappingInformationType").as_deref() {
            Some("ByPolygonVertex") => &new_corners,
            Some("ByPolygon") => &new_polygons,
            _ => continue,
        };
        edits.insert(i, Edit::Nested(layer_edits(layer, layer_spec, source)?));
    }
    Ok(Some((edits, triangles.len(), kept.len())))
}

/// FBX names read `Name\0\x01Geometry`.
fn name_of(geometry: NodeHandle<'_>) -> String {
    geometry
        .attributes()
        .get(1)
        .and_then(|a| a.get_string())
        .and_then(|s| s.split('\0').next())
        .filter(|s| !s.is_empty())
        .map_or_else(|| "Geometry".to_string(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fbxcel::low::FbxVersion;

    type Out<'a> = Writer<Cursor<&'a mut Vec<u8>>>;

    fn ints(w: &mut Out<'_>, name: &str, v: Vec<i32>) {
        {
            let mut a = w.new_node(name).unwrap();
            a.append_arr_i32_from_iter(None, v).unwrap();
        }
        w.close_node().unwrap();
    }

    fn floats(w: &mut Out<'_>, name: &str, v: Vec<f64>) {
        {
            let mut a = w.new_node(name).unwrap();
            a.append_arr_f64_from_iter(None, v).unwrap();
        }
        w.close_node().unwrap();
    }

    fn text(w: &mut Out<'_>, name: &str, s: &str) {
        {
            let mut a = w.new_node(name).unwrap();
            a.append_string_direct(s).unwrap();
        }
        w.close_node().unwrap();
    }

    fn layer(w: &mut Out<'_>, name: &str, mapping: &str, reference: &str) {
        {
            let mut a = w.new_node(name).unwrap();
            a.append_i32(0).unwrap();
        }
        text(w, "MappingInformationType", mapping);
        text(w, "ReferenceInformationType", reference);
    }

    const SEGMENTS: usize = 105;
    const RINGS: usize = 101;

    fn sphere_fbx() -> (Vec<u8>, usize) {
        let mut vertices = vec![0.0, 1.0, 0.0];
        for r in 1..RINGS {
            let theta = std::f64::consts::PI * r as f64 / RINGS as f64;
            for s in 0..SEGMENTS {
                let phi = std::f64::consts::TAU * s as f64 / SEGMENTS as f64;
                vertices.extend([
                    theta.sin() * phi.cos(),
                    theta.cos(),
                    theta.sin() * phi.sin(),
                ]);
            }
        }
        vertices.extend([0.0, -1.0, 0.0]);
        let count = vertices.len() / 3;
        let bottom = (count - 1) as i32;
        let ring = |r: usize, s: usize| (1 + (r - 1) * SEGMENTS + s % SEGMENTS) as i32;

        let mut polygons: Vec<Vec<i32>> = Vec::new();
        for s in 0..SEGMENTS {
            polygons.push(vec![0, ring(1, s + 1), ring(1, s)]);
        }
        for r in 1..RINGS - 1 {
            for s in 0..SEGMENTS {
                polygons.push(vec![
                    ring(r, s),
                    ring(r, s + 1),
                    ring(r + 1, s + 1),
                    ring(r + 1, s),
                ]);
            }
        }
        for s in 0..SEGMENTS {
            polygons.push(vec![bottom, ring(RINGS - 1, s), ring(RINGS - 1, s + 1)]);
        }
        let mut indices = Vec::new();
        for p in &polygons {
            let last = p.len() - 1;
            indices.extend(
                p.iter()
                    .enumerate()
                    .map(|(i, &c)| if i == last { !c } else { c }),
            );
        }
        let normals: Vec<f64> = indices
            .iter()
            .flat_map(|&i| {
                let cp = (if i < 0 { !i } else { i }) as usize;
                vertices[cp * 3..cp * 3 + 3].to_vec()
            })
            .collect();
        let uv: Vec<f64> = (0..count)
            .flat_map(|i| [i as f64 / count as f64, 0.5])
            .collect();
        let uv_index: Vec<i32> = indices
            .iter()
            .map(|&i| if i < 0 { !i } else { i })
            .collect();
        let materials: Vec<i32> = (0..polygons.len() as i32).map(|i| i % 3).collect();

        let mut out = Vec::new();
        {
            let mut w = Writer::new(Cursor::new(&mut out), FbxVersion::V7_4).unwrap();
            w.new_node("Objects").unwrap();
            {
                let mut a = w.new_node("Geometry").unwrap();
                a.append_i64(1).unwrap();
                a.append_string_direct("Sphere\0\u{1}Geometry").unwrap();
                a.append_string_direct("Mesh").unwrap();
            }
            floats(&mut w, "Vertices", vertices);
            ints(&mut w, "PolygonVertexIndex", indices);
            layer(&mut w, "LayerElementNormal", "ByPolygonVertex", "Direct");
            floats(&mut w, "Normals", normals);
            w.close_node().unwrap();
            layer(&mut w, "LayerElementUV", "ByPolygonVertex", "IndexToDirect");
            floats(&mut w, "UV", uv);
            ints(&mut w, "UVIndex", uv_index);
            w.close_node().unwrap();
            layer(&mut w, "LayerElementMaterial", "ByPolygon", "IndexToDirect");
            ints(&mut w, "Materials", materials);
            w.close_node().unwrap();
            w.close_node().unwrap();
            {
                let mut a = w.new_node("Model").unwrap();
                a.append_i64(2).unwrap();
                a.append_string_direct("Cube\0\u{1}Model").unwrap();
            }
            w.new_node("Properties70").unwrap();
            {
                let mut a = w.new_node("P").unwrap();
                a.append_string_direct("Lcl Translation").unwrap();
                a.append_f64(1.5).unwrap();
            }
            w.close_node().unwrap();
            w.close_node().unwrap();
            w.close_node().unwrap();
            w.close_node().unwrap();
            w.finalize_and_flush(&FbxFooter::default()).unwrap();
        }
        (out, count)
    }

    #[test]
    fn a_dense_sphere_is_decimated_with_its_layers_following() {
        let (bytes, vertex_count) = sphere_fbx();
        let (out, decimated) = decimate(&bytes, 20_000).unwrap().unwrap();
        assert_eq!(decimated.len(), 1);
        assert_eq!(decimated[0].name, "Sphere");

        let report = crate::fbx::report(&out);
        assert_eq!(report.meshes.len(), 1);
        let triangles = report.meshes[0].triangles;
        assert!(triangles <= 20_000 && triangles > 19_000, "{triangles}");

        let Ok(AnyTree::V7400(_, tree, _)) = AnyTree::from_seekable_reader(Cursor::new(&out[..]))
        else {
            panic!("not an FBX 7 file");
        };
        let objects = tree.root().children_by_name("Objects").next().unwrap();
        let geometry = objects.children_by_name("Geometry").next().unwrap();
        let get = |layer: &str, name: &str| {
            geometry
                .children_by_name(layer)
                .next()
                .unwrap()
                .children_by_name(name)
                .next()
                .unwrap()
                .attributes()[0]
                .clone()
        };
        let corners = geometry
            .children_by_name("PolygonVertexIndex")
            .next()
            .unwrap()
            .attributes()[0]
            .get_arr_i32()
            .unwrap()
            .len();
        assert_eq!(corners, triangles * 3);
        let normals = get("LayerElementNormal", "Normals");
        assert_eq!(normals.get_arr_f64().unwrap().len(), corners * 3);
        let uv_index = get("LayerElementUV", "UVIndex");
        let uv_index = uv_index.get_arr_i32().unwrap();
        assert_eq!(uv_index.len(), corners);
        assert!(uv_index.iter().all(|&i| (i as usize) < vertex_count));
        let materials = get("LayerElementMaterial", "Materials");
        assert_eq!(materials.get_arr_i32().unwrap().len(), triangles);

        let model = objects.children_by_name("Model").next().unwrap();
        assert_eq!(model.attributes()[0].get_i64(), Some(2));
        let p = model
            .children_by_name("Properties70")
            .next()
            .unwrap()
            .children_by_name("P")
            .next()
            .unwrap();
        assert_eq!(p.attributes()[0].get_string(), Some("Lcl Translation"));
        assert_eq!(p.attributes()[1].get_f64(), Some(1.5));
    }

    #[test]
    fn ascii_text_is_not_a_binary_fbx() {
        assert!(decimate(b"; FBX 7.4.0 project file\n", 10)
            .unwrap()
            .is_none());
    }
}
