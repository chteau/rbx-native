//! A real place end to end, behind an environment variable.

use super::*;

/// A real place through the same streamed meshes and images the editor
/// exports from: every textured mesh comes out with its PNG, and every union
/// whose boolean carved with its own triangles rather than a box. Set
/// `RBX_EXPORT_OUT=<dir>` to keep the `.obj`/`.mtl`/`.png`s, `.gltf` and
/// `.fbx` it wrote, to open in another tool.
#[test]
#[ignore = "needs RBX_EXPORT_FIXTURE=<place> and the network or an asset cache"]
fn a_real_place_exports_its_textures_and_unions() {
    let path = std::env::var("RBX_EXPORT_FIXTURE").expect("RBX_EXPORT_FIXTURE");
    let mut viewer = crate::Headless::load(std::path::Path::new(&path), true).unwrap();
    // Streams in on ticks; quiet for a few seconds means everything landed.
    let mut quiet = 0;
    for _ in 0..1200 {
        viewer.tick(std::time::Duration::from_millis(50));
        quiet = if viewer.pick_meshes_changed() {
            0
        } else {
            quiet + 1
        };
        if quiet > 100 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let dom = crate::read_place(std::path::Path::new(&path)).unwrap();
    let database = ReflectionDatabase::embedded();
    let exported = meshes_of(&dom, &database, &viewer.pick_meshes(), dom.root_refs());

    let textured = exported
        .meshes
        .iter()
        .filter(|m| m.maps.color.is_some())
        .count();
    let unions: Vec<_> = dom
        .root_refs()
        .iter()
        .flat_map(|&root| descendants_of(&dom, root))
        .filter(|&r| dom.get(r).is_some_and(|i| i.class() == "UnionOperation"))
        .filter_map(|r| union_fit(&dom, &database, r).map(|(asset, _)| (r, asset)))
        .collect();
    let carved = unions
        .iter()
        .filter(|(_, asset)| viewer.pick_meshes().get(asset).is_some())
        .count();
    let surfaced = exported
        .meshes
        .iter()
        .filter(|m| m.maps.normal.is_some() || m.maps.metallic_roughness.is_some())
        .count();
    eprintln!(
        "{} parts, {textured} textured, {surfaced} with PBR maps, \
         {} distinct images, {carved}/{} legacy unions carved",
        exported.meshes.len(),
        exported.textures.len(),
        unions.len()
    );
    // Before the asserts, so a place that fails one can still be looked at.
    if let Ok(out) = std::env::var("RBX_EXPORT_OUT") {
        let out = std::path::Path::new(&out);
        for (name, bytes) in obj_files(&exported, "export") {
            std::fs::write(out.join(name), bytes).unwrap();
        }
        for (name, bytes) in gltf_files(&exported, "export") {
            std::fs::write(out.join(name), bytes).unwrap();
        }
        std::fs::write(out.join("export.fbx"), fbx(&exported).unwrap()).unwrap();
    }

    assert!(textured > 0);
    assert!(carved > 0);
}
