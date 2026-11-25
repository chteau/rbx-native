//! A union Studio made keeps its geometry through both writers. Current Studio
//! saves a union's CSG mesh in `SolidMeshHolder` (a NetworkAssetRef: SharedString
//! in binary, `<NetAssetRef>` in XML) and leaves `MeshData2` empty, so dropping
//! or retyping it makes the union invisible or the file unopenable.
//!
//! The fixtures are one union of two 2-stud cubes, exported from Studio as
//! `.rbxm` and trimmed from a Studio-saved `.rbxlx`.

use rbx_dom::{Variant, WeakDom};

const RBXM: &[u8] = include_bytes!("../../../assets/tests/csg/TwoCubesUnion.rbxm");
const RBXMX: &str = include_str!("../../../assets/tests/csg/TwoCubesUnion.rbxmx");

const GEOMETRY: [&str; 3] = ["SolidMeshHolder", "PhysicalConfigData", "ChildData2"];

fn union_props(dom: &WeakDom) -> Vec<(&'static str, Variant)> {
    let union = dom.get(dom.root_refs()[0]).unwrap();
    assert_eq!(union.class(), "UnionOperation");
    GEOMETRY
        .iter()
        .map(|&name| (name, union.properties()[name].clone()))
        .collect()
}

fn assert_shared_geometry(props: &[(&str, Variant)]) {
    for (name, value) in props {
        assert!(
            matches!(value, Variant::Unknown { type_id: 0x1C, raw } if !raw.is_empty()),
            "{name} should be non-empty SharedString bytes, got {value:?}"
        );
    }
}

#[test]
fn binary_union_keeps_its_geometry_through_both_writers() {
    let dom = rbx_binary::deserialize(RBXM).unwrap();
    let before = union_props(&dom);
    assert_shared_geometry(&before);

    let binary = rbx_binary::deserialize(&rbx_binary::serialize(&dom).unwrap()).unwrap();
    assert_eq!(union_props(&binary), before);

    let xml = rbx_xml::serialize(&dom).unwrap();
    assert!(xml.contains("<NetAssetRef name=\"SolidMeshHolder\">"));
    assert!(!xml.contains("<SharedString name=\"SolidMeshHolder\">"));
    assert_eq!(union_props(&rbx_xml::deserialize(&xml).unwrap()), before);
}

#[test]
fn xml_union_keeps_its_geometry_and_tag() {
    let dom = rbx_xml::deserialize(RBXMX).unwrap();
    let before = union_props(&dom);
    assert_shared_geometry(&before);
    // Studio writes the same bytes into either format.
    assert_eq!(union_props(&rbx_binary::deserialize(RBXM).unwrap()), before);

    let xml = rbx_xml::serialize(&dom).unwrap();
    assert!(xml.contains("<NetAssetRef name=\"SolidMeshHolder\">"));
    assert_eq!(union_props(&rbx_xml::deserialize(&xml).unwrap()), before);
}

#[test]
fn legacy_string_solid_mesh_holder_is_left_out_of_xml() {
    let mut dom = WeakDom::new();
    let union = dom.new_instance("UnionOperation", "Rock", None);
    dom.set_property(union, "SolidMeshHolder", Variant::String(String::new()))
        .unwrap();

    let xml = rbx_xml::serialize(&dom).unwrap();

    assert!(!xml.contains("SolidMeshHolder"));
}
