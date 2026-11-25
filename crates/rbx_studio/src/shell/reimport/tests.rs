use rbx_dom::{CFrameData, Content, Variant, Vector3Data, WeakDom};

use super::merge::{reimport, Merged};
use super::store::{new_id, Store};
use super::{id_of, rows, set_id, Watch};

fn at(x: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y: 0.0, z: 0.0 },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    })
}

fn x_of(dom: &WeakDom, part: rbx_dom::Ref) -> f32 {
    match dom.get(part).unwrap().properties().get("CFrame") {
        Some(Variant::CFrame(frame)) => frame.position.x,
        other => panic!("no CFrame: {other:?}"),
    }
}

fn mesh(dom: &mut WeakDom, parent: rbx_dom::Ref, name: &str, id: &str, x: f32) -> rbx_dom::Ref {
    let part = dom.new_instance("MeshPart", name, Some(parent));
    let _ = dom.set_property(part, "MeshId", Variant::Content(Content::Uri(id.into())));
    let _ = dom.set_property(part, "CFrame", at(x));
    part
}

fn prop<'a>(dom: &'a WeakDom, part: rbx_dom::Ref, key: &str) -> Option<&'a Variant> {
    dom.get(part).unwrap().properties().get(key)
}

fn child(dom: &WeakDom, parent: rbx_dom::Ref, name: &str) -> rbx_dom::Ref {
    *dom.get(parent)
        .unwrap()
        .children()
        .iter()
        .find(|&&c| dom.get(c).unwrap().name() == name)
        .unwrap()
}

/// The place's model as first imported, then edited: Body recoloured and
/// given no collision box, Lid left, and the model moved 10 studs along X.
fn placed() -> (WeakDom, rbx_dom::Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Crate", Some(workspace));
    let body = mesh(&mut dom, model, "Body", "rbxassetid://1", 10.0);
    let _ = dom.set_property(body, "Color", Variant::String("red".into()));
    let _ = dom.set_property(body, "CollisionFidelity", Variant::String("Box".into()));
    let _ = dom.set_property(
        body,
        "TextureID",
        Variant::Content(Content::Uri("rbxassetid://7".into())),
    );
    mesh(&mut dom, model, "Lid", "rbxassetid://2", 12.0);
    let _ = dom.set_property(model, "WorldPivot", at(10.0));
    (dom, model)
}

/// Roblox's model of the edited file, at the origin: Body has a new mesh and
/// no texture, Lid is gone, Handle is new.
fn fresh() -> WeakDom {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let body = mesh(&mut dom, model, "Body", "rbxassetid://11", 0.0);
    let _ = dom.set_property(
        body,
        "TextureID",
        Variant::Content(Content::Uri(String::new())),
    );
    mesh(&mut dom, model, "Handle", "rbxassetid://13", 3.0);
    let _ = dom.set_property(model, "WorldPivot", at(0.0));
    dom
}

#[test]
fn a_reimport_updates_matches_adds_new_meshes_and_keeps_the_rest() {
    let (mut dom, model) = placed();
    let database = &rbx_reflection::ReflectionDatabase::embedded();
    let merged = reimport(&mut dom, database, model, &fresh()).unwrap();
    assert_eq!(
        merged,
        Merged {
            updated: 1,
            added: 1
        }
    );

    let body = child(&dom, model, "Body");
    assert_eq!(
        prop(&dom, body, "MeshId"),
        Some(&Variant::Content(Content::Uri("rbxassetid://11".into())))
    );
    assert_eq!(
        prop(&dom, body, "Color"),
        Some(&Variant::String("red".into()))
    );
    assert_eq!(
        prop(&dom, body, "CollisionFidelity"),
        Some(&Variant::String("Box".into()))
    );
    assert_eq!(
        prop(&dom, body, "TextureID"),
        Some(&Variant::Content(Content::Uri("rbxassetid://7".into()))),
        "an untextured incoming mesh keeps the existing texture"
    );
    // The file is laid onto the model where it now stands.
    assert_eq!(x_of(&dom, body), 10.0);
    let handle = child(&dom, model, "Handle");
    assert_eq!(x_of(&dom, handle), 13.0);
    let lid = child(&dom, model, "Lid");
    assert_eq!(
        prop(&dom, lid, "MeshId"),
        Some(&Variant::Content(Content::Uri("rbxassetid://2".into()))),
        "a mesh the file dropped is left alone"
    );
}

#[test]
fn a_surface_appearance_takes_the_files_maps_and_keeps_its_settings() {
    let (mut dom, model) = placed();
    let body = child(&dom, model, "Body");
    let surface = dom.new_instance("SurfaceAppearance", "SurfaceAppearance", Some(body));
    let _ = dom.set_property(
        surface,
        "ColorMap",
        Variant::Content(Content::Uri("old".into())),
    );
    let _ = dom.set_property(surface, "AlphaMode", Variant::String("Overlay".into()));
    let mut file = fresh();
    let fresh_model = file.root_refs()[0];
    let fresh_body = child(&file, fresh_model, "Body");
    let incoming = file.new_instance("SurfaceAppearance", "SurfaceAppearance", Some(fresh_body));
    let _ = file.set_property(
        incoming,
        "ColorMap",
        Variant::Content(Content::Uri("new".into())),
    );
    let _ = file.set_property(incoming, "AlphaMode", Variant::String("Opaque".into()));

    let database = &rbx_reflection::ReflectionDatabase::embedded();
    reimport(&mut dom, database, model, &file).unwrap();
    assert_eq!(
        prop(&dom, surface, "ColorMap"),
        Some(&Variant::Content(Content::Uri("new".into())))
    );
    assert_eq!(
        prop(&dom, surface, "AlphaMode"),
        Some(&Variant::String("Overlay".into()))
    );
}

#[test]
fn an_empty_import_is_refused_and_changes_nothing() {
    let (mut dom, model) = placed();
    let before = dom.get(model).unwrap().children().len();
    let database = &rbx_reflection::ReflectionDatabase::embedded();
    assert!(reimport(&mut dom, database, model, &WeakDom::new()).is_err());
    assert_eq!(dom.get(model).unwrap().children().len(), before);
}

#[test]
fn a_change_reimports_once_the_file_has_held_still() {
    let mut watch = Watch {
        file: "crate.fbx".into(),
        stamp: 1,
        pending: false,
    };
    assert!(!watch.step(1, false), "nothing changed");
    assert!(!watch.step(2, false), "just saved: wait");
    assert!(!watch.step(3, false), "still being written");
    assert!(watch.step(3, false), "settled: reimport");
    assert!(!watch.step(3, false), "once");
    assert!(!watch.step(4, false));
    assert!(!watch.step(4, true), "a reimport is running: hold it");
    assert!(watch.step(4, false), "then go");
}

#[test]
fn the_id_round_trips_beside_other_attributes() {
    let (mut dom, model) = placed();
    assert_eq!(id_of(&dom, model), None);
    let id = new_id();
    set_id(&mut dom, model, Some(&id)).unwrap();
    assert_eq!(id_of(&dom, model), Some(id));
    set_id(&mut dom, model, None).unwrap();
    assert_eq!(id_of(&dom, model), None);
}

#[test]
fn ids_look_like_studios() {
    let id = new_id();
    let groups: Vec<usize> = id.split('-').map(str::len).collect();
    assert_eq!(groups, [8, 4, 4, 4, 12]);
    assert_eq!(&id[14..15], "4");
    assert!(id
        .chars()
        .all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(new_id(), id);
}

#[test]
fn the_device_map_round_trips_and_guards_the_rows() {
    let dir = std::env::temp_dir().join(format!("rbx-native-reimport-{}", std::process::id()));
    let path = dir.join("reimport.json");
    let (mut dom, model) = placed();
    let database = &rbx_reflection::ReflectionDatabase::embedded();
    let mut store = Store::default();
    let none = rows(&dom, database, model, &store);
    assert!(none.model && !none.configured && !none.has_id);

    let id = new_id();
    set_id(&mut dom, model, Some(&id)).unwrap();
    assert!(rows(&dom, database, model, &store).has_id);
    store.set(&id, "/models/crate.fbx".into());
    store.write(&path).unwrap();
    let read = Store::read(&path);
    assert_eq!(read, store);
    assert!(rows(&dom, database, model, &read).configured);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"filepath\""), "{text}");

    let body = child(&dom, model, "Body");
    let part = rows(&dom, database, body, &read);
    assert!(!part.model && !part.configured, "Reimport is a Model's");
    let _ = std::fs::remove_dir_all(dir);
}
