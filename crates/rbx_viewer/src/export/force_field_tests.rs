//! A ForceField: a shell whose look depends on the angle it is seen at, and
//! a mesh whose image is a moving pattern rather than paint.

use super::*;

/// Half-solid at most whatever its `Transparency`, as the viewport caps
/// it; faint face-on through transmission, which a viewer's Fresnel makes
/// more solid towards the rim, where a sheen in its colour lights it.
#[test]
fn a_force_field_is_a_shell_clear_face_on_and_lit_at_its_rim() {
    let mut dom = WeakDom::new();
    let shield = part(&mut dom, "Part", [4.0, 4.0, 4.0]);
    let meshes = Meshes::default().with_kinds(HashMap::from([(shield, Kind::ForceField)]));

    let exported = export(&dom, &meshes, shield);
    let document = document(&exported);

    assert_eq!(exported.meshes[0].color[3], 0.5);
    let material = &document["materials"][0];
    assert!(material.get("alphaMode").is_none());
    let extensions = &material["extensions"];
    let transmission = extensions["KHR_materials_transmission"]["transmissionFactor"]
        .as_f64()
        .unwrap();
    assert!((transmission - (1.0 - 0.5 * 0.4)).abs() < 1e-6);
    assert!(extensions["KHR_materials_sheen"]["sheenColorFactor"].is_array());
    assert!(
        material.get("emissiveTexture").is_none(),
        "a plain part has no pattern"
    );
    let mtl = mtl(&exported.meshes, "x");
    // OBJ has no Fresnel: the shell averaged over every angle, 0.5 × 2/3.
    assert!(mtl.contains("\nd 0.333"), "{mtl}");
}

/// A ForceField mesh's image is its pattern, never paint: what shows of it
/// at the start of the cycle glows and is solid, the rest stays clear.
#[test]
fn a_force_field_meshs_pattern_is_kept_as_a_still() {
    let mut dom = WeakDom::new();
    let shield = part(&mut dom, "MeshPart", [2.0, 2.0, 2.0]);
    dom.set_property(shield, "MeshId", Variant::String("rbxassetid://42".into()))
        .unwrap();
    let vertex = |position: [f32; 3], uv: [f32; 2]| rbx_mesh::Vertex {
        position,
        normal: [0.0, 1.0, 0.0],
        uv,
        color: [255; 4],
    };
    let mesh = rbx_mesh::SkinnedMesh::from(rbx_mesh::Mesh {
        version: (4, 1),
        vertices: vec![
            vertex([-1.0, 0.0, -1.0], [0.0, 0.0]),
            vertex([1.0, 0.0, -1.0], [1.0, 0.0]),
            vertex([0.0, 0.0, 1.0], [0.5, 1.0]),
        ],
        indices: vec![0, 2, 1],
        lods: Vec::new(),
        bounds: rbx_mesh::Aabb {
            min: [-1.0, 0.0, -1.0],
            max: [1.0, 0.0, 1.0],
        },
    });
    // Red 145 sits in the window's centre at the cycle's start (0.567);
    // red 0 is far outside it.
    let image = Arc::new(Image {
        width: 2,
        height: 1,
        pixels: vec![145, 0, 0, 255, 0, 0, 0, 255],
    });
    let meshes = Meshes::new(
        HashMap::from([(AssetRef::Id(42), Arc::new(mesh))]),
        HashMap::from([(shield, image)]),
    )
    .with_kinds(HashMap::from([(shield, Kind::ForceField)]));

    let exported = export(&dom, &meshes, shield);
    let maps = exported.meshes[0].maps;

    assert!(maps.color.is_none(), "the image is never painted on");
    let decode = |index: usize| {
        let mut reader = png::Decoder::new(std::io::Cursor::new(&exported.textures[index]))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut pixels).unwrap();
        pixels
    };
    assert_eq!(
        decode(maps.pattern.unwrap()),
        [255, 255, 255, 255, 0, 0, 0, 255]
    );
    assert_eq!(
        decode(maps.see_through.unwrap()),
        [0, 0, 0, 255, 255, 0, 0, 255]
    );
    let document = document(&exported);
    let material = &document["materials"][0];
    assert!(material["emissiveTexture"].is_object());
    assert!(
        material["extensions"]["KHR_materials_transmission"]["transmissionTexture"].is_object()
    );
}
