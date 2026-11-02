use super::*;

fn node(class: &str, name: &str, children: Vec<Node>) -> Node {
    Node {
        class: class.into(),
        name: name.into(),
        children,
        ..Node::default()
    }
}

fn hat(handle_kids: Vec<Node>) -> Node {
    let mut kids = vec![node("Attachment", "HatAttachment", vec![])];
    kids.extend(handle_kids);
    node("Accessory", "Hat", vec![node("MeshPart", "Handle", kids)])
}

fn messages(f: &[Finding]) -> Vec<&str> {
    f.iter().map(|f| f.message.as_str()).collect()
}

#[test]
fn a_clean_hat_passes() {
    let mut scale = node("StringValue", "AvatarPartScaleType", vec![]);
    scale.value = Some("ProportionsNormal".into());
    let item = hat(vec![
        scale,
        node("Vector3Value", "OriginalSize", vec![]),
        node("SurfaceAppearance", "SurfaceAppearance", vec![]),
    ]);
    assert!(check(&item, AssetType::Hat).is_empty());
}

#[test]
fn strays_are_listed_with_their_descendants_in_studios_words() {
    let mut item = hat(vec![
        node("Part", "Part", vec![node("Decal", "Decal", vec![])]),
        node("Attachment", "HairAttachment", vec![]),
    ]);
    item.children.push(node("Folder", "Folder", vec![]));
    let found = check(&item, AssetType::Hat);
    assert_eq!(
        messages(&found),
        ["The following instances are not part of the approved schema and should be removed: Hat.Handle.Part, Hat.Handle.Part.Decal, Hat.Handle.HairAttachment, Hat.Folder."]
    );
}

#[test]
fn a_special_mesh_only_fits_a_part_handle() {
    let item = hat(vec![node("SpecialMesh", "Mesh", vec![])]);
    assert_eq!(check(&item, AssetType::Hat).len(), 1);
    let mut part = item.clone();
    part.children[0].class = "Part".into();
    assert!(check(&part, AssetType::Hat).is_empty());
}

#[test]
fn a_wrap_layer_on_a_rigid_item_is_named() {
    let found = check(
        &hat(vec![node("WrapLayer", "WrapLayer", vec![])]),
        AssetType::Hat,
    );
    let m = messages(&found);
    assert!(m.contains(
        &"Asset type 'Hat' is not a layered clothing category. It can only be used with rigid accessories."
    ));
    assert!(m.iter().any(|m| m.ends_with("Hat.Handle.WrapLayer.")));
}

#[test]
fn thumbnail_configuration_needs_both_values() {
    let mut item = hat(vec![]);
    item.children
        .push(node("Configuration", "ThumbnailConfiguration", vec![]));
    assert_eq!(
        messages(&check(&item, AssetType::Hat)),
        [
            "Could not find required ObjectValue called ThumbnailCameraTarget inside of Hat.ThumbnailConfiguration",
            "Could not find required CFrameValue called ThumbnailCameraValue inside of Hat.ThumbnailConfiguration",
        ]
    );
}

#[test]
fn scale_type_tags_attributes_and_anchored() {
    let mut scale = node("StringValue", "AvatarPartScaleType", vec![]);
    scale.value = Some("Big".into());
    let mut item = hat(vec![scale]);
    item.attributed = true;
    item.children[0].attributed = true;
    item.children[0].tagged = true;
    item.children[0].anchored = true;
    item.children[0].children[0].attributed = true;
    assert_eq!(
        messages(&check(&item, AssetType::Hat)),
        [
            "The AvatarPartScaleType value in 'Handle' is invalid. Accepted values are Classic, ProportionsSlender, or ProportionsNormal.",
            "Property 'Anchored' on 'Hat.Handle' is invalid. Expected to be = false.",
            "The following instances have tags but marketplace assets cannot have any tags: Hat.Handle",
            "'Hat' (or its descendants) contain Attributes. You need to remove Attributes from the following: Hat, Hat.Handle.HatAttachment",
        ]
    );
}

fn layered(attachment: &str, wrap: Node) -> Node {
    node(
        "Accessory",
        "Brow",
        vec![node(
            "MeshPart",
            "Handle",
            vec![node("Attachment", attachment, vec![]), wrap],
        )],
    )
}

#[test]
fn a_layered_item_without_an_inner_cage_has_no_hsr_data() {
    let item = layered(
        "BodyFrontAttachment",
        node("WrapLayer", "WrapLayer", vec![]),
    );
    assert_eq!(
        messages(&check(&item, AssetType::Shirt)),
        [
            "Missing InnerCage (i.e. invalid ReferenceMeshId) on layered clothing accessory 'Brow'. Make sure you are using a valid meshId and try again.",
            "Missing HSR data for 'Brow.Handle.WrapLayer'. Please try again.",
        ]
    );
}

#[test]
fn eyebrows_need_auto_skin_override_and_an_unscaled_handle() {
    let mut wrap = node("WrapLayer", "WrapLayer", vec![]);
    wrap.reference_mesh = true;
    let mut item = layered("FaceFrontAttachment", wrap);
    item.children[0].scaled = true;
    let found = check(&item, AssetType::Eyebrow);
    assert_eq!(
        messages(&found),
        [
            "EyebrowAccessory must use skinning transfer, please set WrapLayer.AutoSkin to EnabledOverride.",
            "Handle has been scaled, but mesh parts with wrap layers do not support scaling. You need to change the Size property to match the MeshSize property.",
        ]
    );
    item.children[0].scaled = false;
    item.children[0].children[1].auto_skin_override = true;
    assert!(check(&item, AssetType::Eyebrow).is_empty());
}

#[test]
fn shoes_come_as_a_named_pair() {
    let pair = node(
        "Model",
        "Shoes",
        vec![
            node("Accessory", "LeftShoeAccessory", vec![]),
            node("Accessory", "RightShoeAccessory", vec![]),
        ],
    );
    assert!(shoe_pair(&pair).is_none());
    let mut wrong = pair.clone();
    wrong.children[0].name = "LeftShoe".into();
    assert!(shoe_pair(&wrong).is_some());
}
