use std::path::PathBuf;

use base64::Engine;

use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbx_geometry_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn le_f32(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn a_gltf_counts_five_influences_across_both_weight_sets() {
    let mut data = le_f32(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    data.extend([0u8; 12]);
    data.extend(le_f32(&[
        0.25, 0.25, 0.25, 0.15, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    ]));
    data.extend([0u8; 12]);
    data.extend(le_f32(&[
        0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    ]));
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&data)
    );
    let doc = serde_json::json!({
        "asset": {"version": "2.0"},
        "buffers": [{"byteLength": data.len(), "uri": uri}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": 12},
            {"buffer": 0, "byteOffset": 48, "byteLength": 48},
            {"buffer": 0, "byteOffset": 96, "byteLength": 12},
            {"buffer": 0, "byteOffset": 108, "byteLength": 48}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
             "min": [0, 0, 0], "max": [1, 1, 0]},
            {"bufferView": 1, "componentType": 5121, "count": 3, "type": "VEC4"},
            {"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC4"},
            {"bufferView": 3, "componentType": 5121, "count": 3, "type": "VEC4"},
            {"bufferView": 4, "componentType": 5126, "count": 3, "type": "VEC4"}
        ],
        "meshes": [{"name": "Tri", "primitives": [{"attributes": {
            "POSITION": 0, "JOINTS_0": 1, "WEIGHTS_0": 2, "JOINTS_1": 3, "WEIGHTS_1": 4
        }}]}],
        "nodes": [{"mesh": 0, "skin": 0}, {"name": "RBX_Leader"}, {"name": "RBX_Follower"}],
        "skins": [{"joints": [1, 2]}]
    });
    let dir = temp_dir("gltf");
    let file = dir.join("tri.gltf");
    std::fs::write(&file, serde_json::to_vec(&doc).unwrap()).unwrap();
    let geometry = read_geometry(&file).unwrap();
    std::fs::remove_dir_all(dir).unwrap();

    assert_eq!(geometry.meshes.len(), 1);
    let mesh = &geometry.meshes[0];
    assert_eq!(mesh.name, "Tri");
    assert_eq!(mesh.positions.len(), 3);
    assert_eq!(mesh.triangles, [[0, 1, 2]]);
    assert_eq!(mesh.influences, [5, 1, 0]);
    assert_eq!(mesh.bones, ["RBX_Leader", "RBX_Follower"]);
    assert!(mesh.uvs.is_empty());
}

#[test]
fn an_obj_cube_is_twelve_triangles_under_its_model_name() {
    let cube = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
o Cube\nf 1 2 3 4\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";
    let dir = temp_dir("obj");
    let file = dir.join("cube.obj");
    std::fs::write(&file, cube).unwrap();
    let geometry = read_geometry(&file).unwrap();
    std::fs::remove_dir_all(dir).unwrap();

    assert_eq!(geometry.meshes.len(), 1);
    assert_eq!(geometry.meshes[0].name, "Cube");
    assert_eq!(geometry.meshes[0].triangles.len(), 12);
    assert!(geometry.meshes[0].influences.is_empty());
}

#[test]
fn a_binary_fbx_names_the_mesh_after_its_model_and_fans_polygons() {
    use fbxcel::low::FbxVersion;
    use fbxcel::writer::v7400::binary::{FbxFooter, Writer};

    let mut out = Vec::new();
    let mut w = Writer::new(std::io::Cursor::new(&mut out), FbxVersion::V7_4).unwrap();
    w.new_node("Objects").unwrap();
    {
        let mut attrs = w.new_node("Geometry").unwrap();
        attrs.append_i64(1).unwrap();
        attrs.append_string_direct("Quad\0\u{1}Geometry").unwrap();
    }
    {
        let mut attrs = w.new_node("Vertices").unwrap();
        attrs
            .append_arr_f64_from_iter(
                None,
                [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            )
            .unwrap();
    }
    w.close_node().unwrap();
    {
        let mut attrs = w.new_node("PolygonVertexIndex").unwrap();
        attrs.append_arr_i32_from_iter(None, [0, 1, 2, !3]).unwrap();
    }
    w.close_node().unwrap();
    w.close_node().unwrap();
    {
        let mut attrs = w.new_node("Model").unwrap();
        attrs.append_i64(2).unwrap();
        attrs.append_string_direct("Crate\0\u{1}Model").unwrap();
    }
    w.close_node().unwrap();
    w.close_node().unwrap();
    w.new_node("Connections").unwrap();
    {
        let mut attrs = w.new_node("C").unwrap();
        attrs.append_string_direct("OO").unwrap();
        attrs.append_i64(1).unwrap();
        attrs.append_i64(2).unwrap();
    }
    w.close_node().unwrap();
    w.close_node().unwrap();
    w.finalize_and_flush(&FbxFooter::default()).unwrap();

    let dir = temp_dir("fbx");
    let file = dir.join("quad.fbx");
    std::fs::write(&file, out).unwrap();
    let geometry = read_geometry(&file).unwrap();
    std::fs::remove_dir_all(dir).unwrap();

    assert_eq!(geometry.meshes.len(), 1);
    let mesh = &geometry.meshes[0];
    assert_eq!(mesh.name, "Crate");
    assert_eq!(mesh.positions.len(), 4);
    assert_eq!(mesh.triangles, [[0, 1, 2], [0, 2, 3]]);
    assert!(mesh.influences.is_empty() && mesh.uvs.is_empty());
}

#[test]
fn a_skinned_fbx_names_the_bones_its_clusters_bind() {
    use fbxcel::low::FbxVersion;
    use fbxcel::writer::v7400::binary::{FbxFooter, Writer};

    let mut out = Vec::new();
    let mut w = Writer::new(std::io::Cursor::new(&mut out), FbxVersion::V7_4).unwrap();
    w.new_node("Objects").unwrap();
    {
        let mut attrs = w.new_node("Geometry").unwrap();
        attrs.append_i64(1).unwrap();
        attrs.append_string_direct("Tri\0\u{1}Geometry").unwrap();
    }
    {
        let mut attrs = w.new_node("Vertices").unwrap();
        attrs
            .append_arr_f64_from_iter(None, [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])
            .unwrap();
    }
    w.close_node().unwrap();
    {
        let mut attrs = w.new_node("PolygonVertexIndex").unwrap();
        attrs.append_arr_i32_from_iter(None, [0, 1, !2]).unwrap();
    }
    w.close_node().unwrap();
    w.close_node().unwrap();
    {
        let mut attrs = w.new_node("Deformer").unwrap();
        attrs.append_i64(3).unwrap();
        attrs.append_string_direct("\0\u{1}Deformer").unwrap();
        attrs.append_string_direct("Skin").unwrap();
    }
    w.close_node().unwrap();
    for (id, bone, vertex) in [(4, 6, 0), (5, 7, 1)] {
        {
            let mut attrs = w.new_node("Deformer").unwrap();
            attrs.append_i64(id).unwrap();
            attrs.append_string_direct("\0\u{1}SubDeformer").unwrap();
            attrs.append_string_direct("Cluster").unwrap();
        }
        {
            let mut attrs = w.new_node("Indexes").unwrap();
            attrs.append_arr_i32_from_iter(None, [vertex]).unwrap();
        }
        w.close_node().unwrap();
        {
            let mut attrs = w.new_node("Weights").unwrap();
            attrs.append_arr_f64_from_iter(None, [1.0]).unwrap();
        }
        w.close_node().unwrap();
        w.close_node().unwrap();
        let name = if bone == 6 {
            "RBX_Leader"
        } else {
            "RBX_Follower"
        };
        {
            let mut attrs = w.new_node("Model").unwrap();
            attrs.append_i64(bone).unwrap();
            attrs
                .append_string_direct(&format!("{name}\0\u{1}Model"))
                .unwrap();
        }
        w.close_node().unwrap();
    }
    w.close_node().unwrap();
    w.new_node("Connections").unwrap();
    for (child, parent) in [(3, 1), (4, 3), (5, 3), (6, 4), (7, 5)] {
        {
            let mut attrs = w.new_node("C").unwrap();
            attrs.append_string_direct("OO").unwrap();
            attrs.append_i64(child).unwrap();
            attrs.append_i64(parent).unwrap();
        }
        w.close_node().unwrap();
    }
    w.close_node().unwrap();
    w.finalize_and_flush(&FbxFooter::default()).unwrap();

    let dir = temp_dir("fbx_skin");
    let file = dir.join("tri.fbx");
    std::fs::write(&file, out).unwrap();
    let geometry = read_geometry(&file).unwrap();
    std::fs::remove_dir_all(dir).unwrap();

    let mesh = &geometry.meshes[0];
    assert_eq!(mesh.influences, [1, 1, 0]);
    assert_eq!(mesh.bones, ["RBX_Leader", "RBX_Follower"]);
}

#[test]
fn an_ascii_fbx_is_ok_with_a_note_and_no_meshes() {
    let dir = temp_dir("ascii");
    let file = dir.join("a.fbx");
    std::fs::write(&file, "; FBX 7.4.0 project file\n").unwrap();
    let geometry = read_geometry(&file).unwrap();
    std::fs::remove_dir_all(dir).unwrap();

    assert!(geometry.meshes.is_empty());
    assert_eq!(
        geometry.notes,
        ["ASCII FBX files can\u{2019}t be read here; export binary FBX to check them."]
    );
}

#[test]
fn an_unknown_extension_is_refused() {
    assert!(read_geometry(Path::new("a.stl")).is_err());
}
