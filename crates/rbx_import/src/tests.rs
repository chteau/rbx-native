use serde_json::Value;

use super::*;

const CUBE: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
o Cube\nf 1 2 3 4\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

#[test]
fn the_format_comes_from_the_extension_in_any_case() {
    assert_eq!(Format::of(Path::new("a/B.FBX")), Some(Format::Fbx));
    assert_eq!(Format::of(Path::new("a.glb")), Some(Format::Glb));
    assert_eq!(Format::of(Path::new("a.stl")), None);
}

#[test]
fn an_obj_cube_is_twelve_triangles_and_uploads_as_gltf() {
    let (report, upload) = prepare_bytes(Format::Obj, "cube", CUBE.into(), None).unwrap();
    assert_eq!(report.meshes, [MeshInfo { name: "Cube".into(), triangles: 12 }]);
    assert_eq!(upload.content_type, "model/gltf+json");
    assert_eq!(upload.file_name, "cube.gltf");
    // What went up reads back as the same mesh.
    assert_eq!(gltf_file::meshes(&upload.bytes).unwrap(), report.meshes);
}

#[test]
fn an_obj_without_faces_is_refused() {
    let err = prepare_bytes(Format::Obj, "x", b"v 0 0 0\n".to_vec(), None).unwrap_err();
    assert!(err.contains("no faces"), "{err}");
}

#[test]
fn obj_uvs_are_flipped_into_gltf_space() {
    let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nf 1/1 2/2 3/3\n";
    let (_, upload) = prepare_bytes(Format::Obj, "t", obj.into(), None).unwrap();
    let doc: Value = serde_json::from_slice(&upload.bytes).unwrap();
    assert!(doc["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"].is_number());
}

#[test]
fn a_mesh_over_the_limit_is_named_in_the_refusal() {
    let report = Report {
        meshes: vec![
            MeshInfo { name: "ok".into(), triangles: TRIANGLE_LIMIT },
            MeshInfo { name: "big".into(), triangles: TRIANGLE_LIMIT + 1 },
        ],
        unchecked: None,
    };
    let refusal = report.refusal().unwrap();
    assert!(refusal.contains("big (20001 triangles)") && !refusal.contains("ok ("), "{refusal}");
    assert!(Report::default().refusal().is_none());
}

#[test]
fn a_gltf_counts_strips_and_unindexed_triangles() {
    let doc = serde_json::json!({
        "asset": {"version": "2.0"},
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 6, "type": "VEC3",
             "min": [0, 0, 0], "max": [1, 1, 1]},
            {"bufferView": 0, "componentType": 5123, "count": 5, "type": "SCALAR"}
        ],
        "bufferViews": [{"buffer": 0, "byteLength": 4}],
        "buffers": [{"byteLength": 4, "uri": "data:application/octet-stream;base64,AQIDBA=="}],
        "meshes": [
            {"name": "loose", "primitives": [{"attributes": {"POSITION": 0}}]},
            {"primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "mode": 5}]}
        ]
    });
    let (report, _) =
        prepare_bytes(Format::Gltf, "g", serde_json::to_vec(&doc).unwrap(), None).unwrap();
    assert_eq!(
        report.meshes,
        [
            MeshInfo { name: "loose".into(), triangles: 2 },
            MeshInfo { name: "Mesh 1".into(), triangles: 3 },
        ]
    );
}

#[test]
fn a_gltf_side_buffer_becomes_a_data_uri() {
    let dir = std::env::temp_dir().join(format!("rbx_import_side_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("my buf.bin"), [1, 2, 3, 4]).unwrap();
    let doc = br#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"my%20buf.bin"}]}"#;
    let inlined = gltf_file::inline_side_files(doc, Some(&dir)).unwrap();
    let value: Value = serde_json::from_slice(&inlined).unwrap();
    assert_eq!(
        value["buffers"][0]["uri"],
        "data:application/octet-stream;base64,AQIDBA=="
    );
    assert!(gltf_file::inline_side_files(doc, None).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_ascii_fbx_uploads_with_a_note_instead_of_a_count() {
    let (report, upload) =
        prepare_bytes(Format::Fbx, "a", b"; FBX 7.4.0 project file\n".to_vec(), None).unwrap();
    assert!(report.meshes.is_empty() && report.unchecked.is_some());
    assert_eq!(upload.content_type, "model/fbx");
}

#[test]
fn a_binary_fbx_counts_polygons_as_triangles() {
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
        let mut attrs = w.new_node("PolygonVertexIndex").unwrap();
        // A quad (2 triangles) then a triangle.
        attrs.append_arr_i32_from_iter(None, [0, 1, 2, !3, 0, 2, !3]).unwrap();
    }
    w.close_node().unwrap();
    w.close_node().unwrap();
    w.close_node().unwrap();
    w.finalize_and_flush(&FbxFooter::default()).unwrap();
    let (report, _) = prepare_bytes(Format::Fbx, "q", out, None).unwrap();
    assert_eq!(report.meshes, [MeshInfo { name: "Quad".into(), triangles: 3 }]);
}
