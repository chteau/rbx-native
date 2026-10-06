//! The `Object` source: which properties take one, which targets they
//! refuse, and that what a pick writes survives a save and a reload.

use rbx_dom::{Content, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::{object_class, object_text};
use crate::properties::edit::commit;
use crate::save::Format;

/// A Decal holding an empty `TextureContent` (how a newer file stores it),
/// an `EditableImage` and an `EditableMesh` to pick, and a `Part` that is
/// neither.
struct Place {
    dom: WeakDom,
    decal: Ref,
    image: Ref,
    mesh: Ref,
    part: Ref,
}

fn place() -> Place {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Part", Some(workspace));
    let decal = dom.new_instance("Decal", "Decal", Some(part));
    dom.set_property(decal, "TextureContent", Variant::Content(Content::None))
        .unwrap();
    let image = dom.new_instance("EditableImage", "Image", Some(workspace));
    let mesh = dom.new_instance("EditableMesh", "Mesh", Some(workspace));
    Place {
        dom,
        decal,
        image,
        mesh,
        part,
    }
}

fn texture(place: &Place) -> Option<&Variant> {
    place
        .dom
        .get(place.decal)
        .unwrap()
        .properties()
        .get("TextureContent")
}

/// A reload renumbers referents, so instances are found again by name.
fn find(dom: &WeakDom, refs: &[Ref], name: &str) -> Option<Ref> {
    refs.iter().find_map(|&r| {
        let instance = dom.get(r)?;
        (instance.name() == name)
            .then_some(r)
            .or_else(|| find(dom, instance.children(), name))
    })
}

fn pick(place: &mut Place, name: &str, target: Ref) -> Result<Option<Variant>, String> {
    let db = ReflectionDatabase::embedded();
    commit(&mut place.dom, &db, place.decal, name, &object_text(target))
}

#[test]
fn each_documented_property_takes_its_documented_object() {
    let db = ReflectionDatabase::embedded();
    let class = |class, name| object_class(&db, class, name);
    assert_eq!(class("Decal", "TextureContent"), Some("EditableImage"));
    assert_eq!(class("ImageLabel", "ImageContent"), Some("EditableImage"));
    assert_eq!(class("MeshPart", "TextureContent"), Some("EditableImage"));
    assert_eq!(class("MeshPart", "MeshContent"), Some("EditableMesh"));
    assert_eq!(class("VideoFrame", "VideoContent"), Some("VideoCapture"));
    assert_eq!(
        class("SurfaceAppearance", "NormalMapContent"),
        Some("EditableImage")
    );
    // Inherited from the declaring class.
    assert_eq!(class("SpecialMesh", "MeshContent"), Some("EditableMesh"));
    assert_eq!(
        class("SpecialMesh", "TextureContent"),
        Some("EditableImage")
    );
    assert_eq!(class("WrapLayer", "CageMeshContent"), Some("EditableMesh"));
}

#[test]
fn an_image_property_the_docs_do_not_extend_to_objects_holds_a_uri_only() {
    let db = ReflectionDatabase::embedded();
    let class = |class, name| object_class(&db, class, name);
    // "does not support EditableImage objects" / "only supports asset URIs".
    assert_eq!(class("Shirt", "ShirtTemplateContent"), None);
    assert_eq!(class("Pants", "PantsTemplateContent"), None);
    assert_eq!(class("Mouse", "IconContent"), None);
    assert_eq!(class("TerrainDetail", "NormalMapContent"), None);
    // Silent on objects, so not guessed at.
    assert_eq!(class("ImageButton", "HoverImageContent"), None);
    assert_eq!(class("Decal", "ColorMapContent"), None);
    assert_eq!(class("Sky", "SkyboxUpContent"), None);
    // Same name as a table entry, on a class that does not declare it.
    assert_eq!(class("Beam", "TextureContent"), None);
}

#[test]
fn a_contentid_or_a_non_visual_content_takes_no_object() {
    let db = ReflectionDatabase::embedded();
    let class = |class, name| object_class(&db, class, name);
    // Saved as Strings, which have no spelling for an object.
    assert_eq!(class("Decal", "Texture"), None);
    assert_eq!(class("MeshPart", "MeshId"), None);
    // Engine-managed, and no documented object for sound or animation.
    assert_eq!(class("Decal", "TexturePackContent"), None);
    assert_eq!(class("Sound", "AudioContent"), None);
    assert_eq!(class("Animation", "AnimationContent"), None);
}

#[test]
fn a_pick_of_the_right_class_writes_an_object() {
    let mut place = place();
    let image = place.image;
    let previous = pick(&mut place, "TextureContent", image).unwrap();
    assert_eq!(previous, Some(Variant::Content(Content::None)));
    assert_eq!(
        texture(&place),
        Some(&Variant::Content(Content::Object(image)))
    );
}

#[test]
fn a_pick_of_the_wrong_class_is_refused_and_writes_nothing() {
    let mut place = place();
    let (part, mesh) = (place.part, place.mesh);
    assert_eq!(
        pick(&mut place, "TextureContent", part),
        Err("TextureContent takes an EditableImage, not a Part".to_owned())
    );
    assert_eq!(
        pick(&mut place, "TextureContent", mesh),
        Err("TextureContent takes an EditableImage, not an EditableMesh".to_owned())
    );
    assert_eq!(texture(&place), Some(&Variant::Content(Content::None)));
}

#[test]
fn a_contentid_refuses_an_object_whichever_shape_it_is_stored_in() {
    let mut place = place();
    let (decal, image) = (place.decal, place.image);
    for stored in [
        Variant::String("rbxassetid://1".into()),
        Variant::Content(Content::None),
    ] {
        place
            .dom
            .set_property(decal, "Texture", stored.clone())
            .unwrap();
        assert!(pick(&mut place, "Texture", image).is_err());
        assert_eq!(
            place.dom.get(decal).unwrap().properties().get("Texture"),
            Some(&stored),
            "a refused object must not clear the asset id"
        );
    }
}

#[test]
fn clearing_an_object_leaves_none() {
    let mut place = place();
    let image = place.image;
    pick(&mut place, "TextureContent", image).unwrap();
    let db = ReflectionDatabase::embedded();
    commit(&mut place.dom, &db, place.decal, "TextureContent", "").unwrap();
    assert_eq!(texture(&place), Some(&Variant::Content(Content::None)));
}

#[test]
fn a_row_that_takes_an_object_is_a_content_row_in_either_mode() {
    use crate::properties::{EditKind, Properties};

    let mut place = place();
    let (decal, image) = (place.decal, place.image);
    let properties = Properties::new(ReflectionDatabase::embedded());
    let row = |dom: &WeakDom, selection: &[Ref], name: &str| {
        properties
            .rows(dom, selection, None)
            .into_iter()
            .find(|row| row.name == name)
            .unwrap()
    };

    let uri = row(&place.dom, &[decal], "TextureContent");
    assert_eq!(
        uri.edit,
        Some(EditKind::Content {
            object: false,
            text: String::new()
        })
    );
    // A ContentId is a plain URI field, with no pick beside it.
    assert!(matches!(
        row(&place.dom, &[decal], "Texture").edit,
        Some(EditKind::Text(_))
    ));

    pick(&mut place, "TextureContent", image).unwrap();
    let object = row(&place.dom, &[decal], "TextureContent");
    assert_eq!(object.value, "Image");
    assert_eq!(
        object.edit,
        Some(EditKind::Content {
            object: true,
            text: object_text(image)
        })
    );

    // Beside a Decal holding none, the row is an empty URI field whose pick
    // names the object for both.
    let parent = place.dom.parent(decal);
    let other = place.dom.new_instance("Decal", "Other", parent);
    place
        .dom
        .set_property(other, "TextureContent", Variant::Content(Content::None))
        .unwrap();
    let mixed = row(&place.dom, &[decal, other], "TextureContent");
    assert!(mixed.mixed);
    assert_eq!(
        mixed.edit,
        Some(EditKind::Content {
            object: false,
            text: String::new()
        })
    );
}

#[test]
fn a_picked_object_survives_a_binary_and_an_xml_save() {
    let mut place = place();
    let image = place.image;
    pick(&mut place, "TextureContent", image).unwrap();

    for format in [Format::Binary, Format::Xml] {
        let bytes = format.encode(&place.dom).unwrap();
        let reloaded = match format {
            Format::Binary => rbx_binary::deserialize(&bytes).unwrap(),
            Format::Xml => rbx_xml::deserialize(std::str::from_utf8(&bytes).unwrap()).unwrap(),
        };
        let named = |name: &str| find(&reloaded, reloaded.root_refs(), name).unwrap();
        let target = match reloaded
            .get(named("Decal"))
            .unwrap()
            .properties()
            .get("TextureContent")
        {
            Some(Variant::Content(Content::Object(target))) => *target,
            other => panic!("{format:?} reloaded TextureContent as {other:?}"),
        };
        assert_eq!(target, named("Image"), "{format:?} lost the target");
        assert_eq!(reloaded.get(target).unwrap().class(), "EditableImage");
    }
}
