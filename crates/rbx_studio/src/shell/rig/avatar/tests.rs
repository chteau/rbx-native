use rbx_dom::Vector3Data;

use super::*;
use crate::shell::rig::build_rig;

fn avatar_json(kind: &str, extra: &str) -> Vec<u8> {
    format!(
        r#"{{"scales":{{"height":1.0,"width":0.9,"head":1.0,"depth":0.9,"proportion":0.5,"bodyType":1.0}},"playerAvatarType":"{kind}","bodyColors":{{"headColorId":24,"torsoColorId":23,"rightArmColorId":24,"leftArmColorId":24,"rightLegColorId":119,"leftLegColorId":119}},"assets":[{{"id":1,"name":"Cap","assetType":{{"id":8,"name":"Hat"}}}},{{"id":2,"name":"Shirt","assetType":{{"id":11,"name":"Shirt"}}}},{{"id":3,"name":"Torso","assetType":{{"id":27,"name":"Torso"}}}}{extra}]}}"#
    )
    .into_bytes()
}

fn avatar_r15() -> Avatar {
    Avatar::from_json(&avatar_json("R15", "")).unwrap()
}

fn uri(text: &str) -> Variant {
    Variant::Content(Content::Uri(text.into()))
}

fn vec3(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

fn attach(dom: &mut WeakDom, parent: Ref, name: &str, at: V3) {
    let node = dom.new_instance("Attachment", name, Some(parent));
    set(dom, node, "CFrame", Variant::CFrame(Cf::at(at).data()));
}

fn cap() -> WeakDom {
    let mut dom = WeakDom::new();
    let hat = dom.new_instance("Accessory", "Cap", None);
    let handle = dom.new_instance("Part", "Handle", Some(hat));
    attach(&mut dom, handle, "HatAttachment", [0., -0.5, 0.]);
    dom
}

fn shirt() -> WeakDom {
    let mut dom = WeakDom::new();
    let node = dom.new_instance("Shirt", "Shirt", None);
    set(&mut dom, node, "ShirtTemplate", uri("rbxassetid://55"));
    dom
}

fn worn_of(avatar: &Avatar) -> Vec<Worn> {
    avatar
        .assets
        .iter()
        .filter(|a| a.id < 3)
        .map(|a| Worn {
            asset: a.clone(),
            dom: Ok(if a.id == 1 { cap() } else { shirt() }),
        })
        .collect()
}

fn worn_asset(id: u64, kind: u32, dom: WeakDom) -> Worn {
    Worn {
        asset: AvatarAsset {
            id,
            name: format!("Asset {id}"),
            asset_type: rbx_cloud::AssetType {
                id: kind,
                name: format!("Type {kind}"),
            },
        },
        dom: Ok(dom),
    }
}

fn used(fates: &[Fate]) -> usize {
    fates.iter().filter(|f| **f == Fate::Used).count()
}

fn notes_of(avatar: &Avatar, worn: &[Worn]) -> Vec<String> {
    let mut options = options_for(avatar, JointStyle::AnimationConstraint, [0.; 3], None);
    let early = apply_packages(&mut options, worn);
    let mut dom = WeakDom::new();
    let root = dom.new_instance("DataModel", "Game", None);
    let rig = build_rig(&mut dom, &options, root);
    let late = dress(&mut dom, rig, worn);
    settle(avatar, worn, &merge(early, late)).1
}

fn rig_of(avatar: &Avatar) -> (WeakDom, Ref, RigOptions) {
    let mut dom = WeakDom::new();
    let root = dom.new_instance("DataModel", "Game", None);
    let options = options_for(avatar, JointStyle::AnimationConstraint, [0.; 3], None);
    let rig = build_rig(&mut dom, &options, root);
    (dom, rig, options)
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rig-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn package(parts: &[(&str, u64, &[&str])]) -> WeakDom {
    let mut dom = WeakDom::new();
    let folder = dom.new_instance("Folder", "R15Fixed", None);
    for (name, mesh, rig) in parts {
        let part = dom.new_instance("MeshPart", name, Some(folder));
        set(
            &mut dom,
            part,
            "MeshId",
            uri(&format!("rbxassetid://{mesh}")),
        );
        set(&mut dom, part, "InitialSize", vec3(1., 1., 1.));
        for attachment in *rig {
            attach(&mut dom, part, attachment, [0.; 3]);
        }
    }
    dom
}

#[test]
fn options_follow_the_avatar() {
    let o = options_for(
        &avatar_r15(),
        JointStyle::AnimationConstraint,
        [1., 2., 3.],
        None,
    );
    assert_eq!(o.rig_type, RigType::R15);
    assert_eq!(o.scales.unwrap().width, 0.9);
    assert_eq!(o.colors.left_leg, BrickColor::from_number(119).unwrap().rgb);
    assert_eq!(o.feet, [1., 2., 3.]);
}

#[test]
fn a_cap_is_welded_to_the_head_attachment_and_clothing_is_copied() {
    let avatar = avatar_r15();
    let (mut dom, rig, _) = rig_of(&avatar);
    assert_eq!(used(&dress(&mut dom, rig, &worn_of(&avatar))), 2);
    let all = descendants(&dom, rig);
    let weld = all
        .iter()
        .copied()
        .find(|&r| dom.get(r).is_some_and(|i| i.name() == "AccessoryWeld"))
        .unwrap();
    let Some(Variant::Ref(part1)) = dom.get(weld).unwrap().properties().get("Part1") else {
        panic!("weld has no Part1");
    };
    assert_eq!(dom.get(*part1).unwrap().name(), "Head");
    let handle = dom.parent(weld).unwrap();
    assert_eq!(dom.get(handle).unwrap().name(), "Handle");
    // The cap's attachment sits 0.5 under its handle's centre, on the head's
    // HatAttachment: the handle centre is half a stud above that.
    let head = cframe_of(&dom, *part1);
    let hat_at = child_named(&dom, *part1, "HatAttachment").unwrap();
    let wanted = head.mul(&cframe_of(&dom, hat_at)).p[1] + 0.5;
    assert!((cframe_of(&dom, handle).p[1] - wanted).abs() < 1e-3);
    // build_rig makes no Shirt, so the worn one is created.
    let shirts = children_of(&dom, rig, "Shirt");
    assert_eq!(shirts.len(), 1);
    assert_eq!(
        dom.get(shirts[0])
            .unwrap()
            .properties()
            .get("ShirtTemplate"),
        Some(&uri("rbxassetid://55"))
    );
}

fn children_of(dom: &WeakDom, parent: Ref, class: &str) -> Vec<Ref> {
    dom.get(parent)
        .unwrap()
        .children()
        .iter()
        .copied()
        .filter(|&c| dom.get(c).is_some_and(|i| i.class() == class))
        .collect()
}

#[test]
fn a_layered_accessory_keeps_its_wrap_layer_and_sits_on_its_attachment() {
    let avatar = avatar_r15();
    let (mut dom, rig, _) = rig_of(&avatar);
    let mut jacket = WeakDom::new();
    let accessory = jacket.new_instance("Accessory", "Jacket", None);
    let handle = jacket.new_instance("MeshPart", "Handle", Some(accessory));
    attach(&mut jacket, handle, "BodyFrontAttachment", [0.; 3]);
    let layer = jacket.new_instance("WrapLayer", "WrapLayer", Some(handle));
    set(&mut jacket, layer, "ReferenceMeshId", uri("rbxassetid://9"));
    let fates = dress(&mut dom, rig, &[worn_asset(9, 68, jacket)]);
    assert!(matches!(fates[..], [Fate::Approximated(_)]));
    let handle = descendants(&dom, rig)
        .into_iter()
        .find(|&r| dom.get(r).is_some_and(|i| i.name() == "Handle"))
        .unwrap();
    assert_eq!(children_of(&dom, handle, "WrapLayer").len(), 1);
    let weld = child_named(&dom, handle, "AccessoryWeld").unwrap();
    let Some(Variant::Ref(part)) = dom.get(weld).unwrap().properties().get("Part1") else {
        panic!("weld has no Part1");
    };
    assert_eq!(dom.get(*part).unwrap().name(), "UpperTorso");
}

#[test]
fn what_is_not_applied_is_named() {
    let extra = r#",{"id":4,"name":"Smile","assetType":{"id":18,"name":"Face"}},{"id":5,"name":"Head","assetType":{"id":79,"name":"DynamicHead"}}"#;
    let r6 = Avatar::from_json(&avatar_json("R6", extra)).unwrap();
    // Every used asset has a download, as `gather` makes them.
    let everything = |avatar: &Avatar| -> Vec<Worn> {
        avatar
            .assets
            .iter()
            .filter(|a| is_used(a.asset_type.id))
            .map(|a| Worn {
                asset: a.clone(),
                dom: Ok(match a.id {
                    1 => cap(),
                    2 => shirt(),
                    _ => WeakDom::new(),
                }),
            })
            .collect()
    };
    let notes = notes_of(&r6, &everything(&r6));
    for name in [
        "Torso (id 3, ",
        "Smile (id 4, ",
        "Head (id 5, DynamicHead): ",
    ] {
        assert!(
            notes.iter().any(|n| n.starts_with(name)),
            "{name}: {notes:?}"
        );
    }
    assert!(!notes
        .iter()
        .any(|n| n.starts_with("Cap") || n.starts_with("Shirt")));
    // A failed download is reported by name, id and type.
    let r15 = Avatar::from_json(&avatar_json("R15", extra)).unwrap();
    let mut worn = everything(&r15);
    worn[0].dom = Err("HTTP 404".into());
    let notes = notes_of(&r15, &worn);
    assert!(notes[0].starts_with("Cap (id 1, "), "{notes:?}");
    assert!(notes[0].ends_with("HTTP 404"), "{notes:?}");
}

fn mock_dir(name: &str, avatar: &[u8]) -> std::path::PathBuf {
    let dir = temp_dir(name);
    std::fs::write(dir.join("156.json"), avatar).unwrap();
    std::fs::write(dir.join("me.json"), avatar).unwrap();
    for (id, dom) in [(1, cap()), (2, shirt()), (3, WeakDom::new())] {
        std::fs::write(
            dir.join(format!("{id}.rbxm")),
            rbx_binary::serialize(&dom).unwrap(),
        )
        .unwrap();
    }
    dir
}

#[test]
fn the_mock_serves_a_player_and_me_from_a_directory() {
    let dir = mock_dir("serves", &avatar_json("R15", ""));
    let path = dir.to_str().unwrap();
    for user in [Some(156), None] {
        let fetched = mock_fetch(path, user).unwrap();
        assert_eq!(fetched.worn.len(), 3);
        assert!(fetched.worn.iter().all(|w| w.dom.is_ok()));
    }
    // An id with no file is a nonexistent user, and nothing is fetched.
    let err = mock_fetch(path, Some(7)).err().unwrap();
    assert!(err.contains("No Roblox user has the id 7"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_mock_simulates_each_failure() {
    let dir = mock_dir("fails", &avatar_json("R15", ""));
    let path = dir.to_str().unwrap();
    for (status, message) in [
        ("404", "No Roblox user has the id 156"),
        ("400", "No Roblox user has the id 156"),
        ("429", "rate limiting"),
        ("network", "Couldn\u{2019}t reach Roblox"),
        ("500", "banned"),
    ] {
        std::fs::write(dir.join("156.status"), status).unwrap();
        let err = mock_fetch(path, Some(156)).err().unwrap();
        assert!(err.contains(message), "{status}: {err}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_clothing_asset_the_mock_refuses_is_named_in_its_own_note() {
    let dir = mock_dir("clothing-401", &avatar_json("R15", ""));
    std::fs::write(dir.join("2.status"), "401").unwrap();
    let fetched = mock_fetch(dir.to_str().unwrap(), Some(156)).unwrap();
    let shirt = fetched.worn.iter().find(|w| w.asset.id == 2).unwrap();
    assert!(shirt.dom.as_ref().err().unwrap().starts_with("HTTP 401"));
    let fates = vec![Fate::Used; fetched.worn.len()];
    let (used, notes) = settle(&fetched.avatar, &fetched.worn, &fates);
    assert_eq!(used, 2);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].starts_with(
            "Shirt (id 2, Shirt): HTTP 401: Roblox serves this only to a signed-in account"
        ),
        "{notes:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_avatar_that_wears_nothing_is_refused() {
    let dir = temp_dir("empty");
    let full = String::from_utf8(avatar_json("R15", "")).unwrap();
    let empty = full.split(r#""assets""#).next().unwrap().to_owned() + r#""assets":[]}"#;
    std::fs::write(dir.join("12.json"), empty).unwrap();
    let err = mock_fetch(dir.to_str().unwrap(), Some(12)).err().unwrap();
    assert!(
        err.contains("user 12") && err.contains("nothing to import"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_single_file_mock_loads_assets_beside_it() {
    let dir = mock_dir("single", &avatar_json("R15", ""));
    let file = dir.join("156.json");
    let fetched = mock_fetch(file.to_str().unwrap(), Some(99)).unwrap();
    assert_eq!(fetched.worn.len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unknown_colour_id_falls_back_to_medium_stone_grey() {
    let mut avatar = avatar_r15();
    avatar.body_colors.left_leg = 999_999;
    let o = options_for(&avatar, JointStyle::AnimationConstraint, [0.; 3], None);
    assert_eq!(o.colors.left_leg, [163, 162, 165]);
}

#[test]
fn the_rig_type_override_converts_in_both_directions() {
    let mut r15 = avatar_r15();
    let joints = JointStyle::AnimationConstraint;
    assert_eq!(own_type(&r15), RigType::R15);
    assert_eq!(
        options_for(&r15, joints, [0.; 3], None).rig_type,
        RigType::R15
    );
    assert_eq!(
        options_for(&r15, joints, [0.; 3], Some(RigType::R6)).rig_type,
        RigType::R6
    );
    r15.avatar_type = "R6".into();
    assert_eq!(own_type(&r15), RigType::R6);
    assert_eq!(
        options_for(&r15, joints, [0.; 3], Some(RigType::R15)).rig_type,
        RigType::R15
    );
    // Both builds dress without panicking.
    for forced in [RigType::R6, RigType::R15] {
        let mut dom = WeakDom::new();
        let root = dom.new_instance("DataModel", "Game", None);
        let o = options_for(&r15, joints, [0.; 3], Some(forced));
        let rig = build_rig(&mut dom, &o, root);
        assert_eq!(used(&dress(&mut dom, rig, &worn_of(&r15))), 2, "{forced:?}");
    }
}

#[test]
fn download_errors_are_specific() {
    let auth = CloudError::AuthRequired { asset_id: 1 };
    assert!(download_error(&auth, false, None).contains("no Open Cloud key"));
    assert!(download_error(&auth, false, None).contains("legacy-asset:manage"));
    let forbidden = CloudError::Http {
        status: 403,
        url: String::new(),
    };
    assert!(download_error(&forbidden, true, None).contains("stored Open Cloud key"));
    let gone = CloudError::Http {
        status: 404,
        url: String::new(),
    };
    assert!(download_error(&gone, false, None).contains("deleted or moderated"));
    assert!(is_transient(&CloudError::RateLimited { retry_after: None }));
    assert!(!is_transient(&gone));
}

#[test]
fn errors_name_what_went_wrong() {
    let rate = user_error(&CloudError::RateLimited { retry_after: None }, 1);
    assert!(rate.contains("429") && !rate.contains("retry in"), "{rate}");
    let unreadable = user_error(&CloudError::UnexpectedShape("assets".into()), 1);
    assert!(unreadable.contains("can\u{2019}t be read"), "{unreadable}");
}

#[test]
fn a_whole_package_replaces_the_stock_part_and_a_partial_one_does_not() {
    let avatar = avatar_r15();
    let (_, _, mut options) = rig_of(&avatar);
    let stock_hand = options.pieces["LeftHand"].mesh;
    let worn = [
        worn_asset(10, 27, package(&[("Head", 777, &["NeckRigAttachment"])])),
        // Missing the wrist attachment: its joint would not meet.
        worn_asset(11, 28, package(&[("LeftHand", 888, &[])])),
    ];
    assert_eq!(used(&apply_packages(&mut options, &worn)), 1);
    assert_eq!(options.pieces["Head"].mesh, 777);
    assert_eq!(options.pieces["LeftHand"].mesh, stock_hand);
    // The swapped mesh reaches the built rig.
    let mut dom = WeakDom::new();
    let root = dom.new_instance("DataModel", "Game", None);
    let rig = build_rig(&mut dom, &options, root);
    let head = child_named(&dom, rig, "Head").unwrap();
    assert_eq!(content_id(&dom, head, "MeshId"), Some(777));
}

#[test]
fn a_fixed_copy_is_preferred_over_the_artist_copy() {
    let mut dom = package(&[("Head", 1, &["NeckRigAttachment"])]);
    let loose = dom.new_instance("Folder", "R15ArtistIntent", None);
    let head = dom.new_instance("MeshPart", "Head", Some(loose));
    set(&mut dom, head, "MeshId", uri("rbxassetid://2"));
    set(&mut dom, head, "InitialSize", vec3(1., 1., 1.));
    attach(&mut dom, head, "NeckRigAttachment", [0.; 3]);
    assert_eq!(pieces_in(&dom)["Head"].mesh, 1);
}

#[test]
fn a_dynamic_head_swaps_the_head_mesh_and_texture() {
    let avatar = avatar_r15();
    let (_, _, mut options) = rig_of(&avatar);
    let mut source = WeakDom::new();
    let mesh = source.new_instance("MeshPart", "Head", None);
    set(&mut source, mesh, "MeshId", uri("rbxassetid://888"));
    set(&mut source, mesh, "TextureID", uri("rbxassetid://999"));
    let worn = [worn_asset(12, DYNAMIC_HEAD, source)];
    assert_eq!(used(&apply_packages(&mut options, &worn)), 1);
    assert!(
        !options.pieces["Head"].face,
        "a dynamic head has its own face"
    );
    assert_eq!(options.pieces["Head"].mesh, 888);
    assert_eq!(options.pieces["Head"].texture, Some(999));
}

#[test]
fn an_r6_rig_takes_no_packages() {
    let r6 = Avatar::from_json(&avatar_json("R6", "")).unwrap();
    let mut options = options_for(&r6, JointStyle::Motor6D, [0.; 3], None);
    let worn = [worn_asset(
        10,
        27,
        package(&[("Head", 777, &["NeckRigAttachment"])]),
    )];
    assert_eq!(used(&apply_packages(&mut options, &worn)), 0);
}

fn animations(id: u64) -> WeakDom {
    let mut dom = WeakDom::new();
    let folder = dom.new_instance("Folder", "R15Anim", None);
    let idle = dom.new_instance("StringValue", "idle", Some(folder));
    let anim = dom.new_instance("Animation", "Animation1", Some(idle));
    set(
        &mut dom,
        anim,
        "AnimationId",
        uri(&format!("rbxassetid://{id}")),
    );
    let other = dom.new_instance("StringValue", "unknown", Some(folder));
    dom.new_instance("Animation", "Animation1", Some(other));
    dom
}

#[test]
fn an_animation_package_replaces_the_states_it_names() {
    for (kind, id) in [("R15", 4242), ("R6", 4243)] {
        let avatar = Avatar::from_json(&avatar_json(kind, "")).unwrap();
        let (mut dom, rig, _) = rig_of(&avatar);
        let worn = [worn_asset(20, 51, animations(id))];
        assert_eq!(used(&dress(&mut dom, rig, &worn)), 1);
        let animate = child_named(&dom, rig, "Animate").unwrap();
        let idle = child_named(&dom, animate, "idle").unwrap();
        let kids = dom.get(idle).unwrap().children().to_vec();
        assert_eq!(kids.len(), 1, "{kind}: the stock idle animations are gone");
        assert_eq!(
            dom.get(kids[0]).unwrap().properties().get("AnimationId"),
            Some(&uri(&format!("rbxassetid://{id}")))
        );
        // A state the script does not have is not invented.
        assert!(child_named(&dom, animate, "unknown").is_none());
    }
}

#[test]
fn the_humanoid_description_records_the_avatars_ids() {
    let avatar = Avatar::from_json(&avatar_json(
        "R15",
        r#",{"id":77,"name":"Hair","assetType":{"id":41,"name":"HairAccessory"}},{"id":88,"name":"Leg","assetType":{"id":30,"name":"LeftLeg"}},{"id":99,"name":"Face","assetType":{"id":18,"name":"Face"}}"#,
    ))
    .unwrap();
    for kind in [RigType::R6, RigType::R15] {
        let mut dom = WeakDom::new();
        let root = dom.new_instance("DataModel", "Game", None);
        let o = options_for(
            &avatar,
            JointStyle::AnimationConstraint,
            [0.; 3],
            Some(kind),
        );
        let rig = build_rig(&mut dom, &o, root);
        describe_avatar(&mut dom, rig, &avatar);
        let humanoid = child_named(&dom, rig, "Humanoid").unwrap();
        let d = child_named(&dom, humanoid, "HumanoidDescription").unwrap();
        let props = dom.get(d).unwrap().properties();
        assert_eq!(props.get("Shirt"), Some(&Variant::Int64(2)), "{kind:?}");
        assert_eq!(props.get("Face"), Some(&Variant::Int64(99)));
        let kids = dom.get(d).unwrap().children().to_vec();
        let ids = |class: &str| -> Vec<(Option<Variant>, Option<Variant>)> {
            kids.iter()
                .filter_map(|&k| dom.get(k))
                .filter(|i| i.class() == class)
                .map(|i| {
                    (
                        i.properties().get("AssetId").cloned(),
                        i.properties()
                            .get("BodyPart")
                            .or(i.properties().get("AccessoryType"))
                            .cloned(),
                    )
                })
                .collect()
        };
        let parts = ids("BodyPartDescription");
        assert!(parts.contains(&(Some(Variant::Int64(3)), Some(Variant::Enum(1)))));
        assert!(parts.contains(&(Some(Variant::Int64(88)), Some(Variant::Enum(4)))));
        assert_eq!(
            ids("AccessoryDescription"),
            vec![
                (Some(Variant::Int64(1)), Some(Variant::Enum(1))),
                (Some(Variant::Int64(77)), Some(Variant::Enum(2)))
            ]
        );
    }
}

fn clothing(class: &str, key: &str, value: &str) -> WeakDom {
    let mut dom = WeakDom::new();
    let node = dom.new_instance(class, class, None);
    set(&mut dom, node, key, uri(value));
    dom
}

#[test]
fn legacy_template_urls_become_asset_ids_and_a_tshirt_gets_its_own_graphic() {
    let avatar = avatar_r15();
    let (mut dom, rig, _) = rig_of(&avatar);
    let worn = vec![
        worn_asset(
            2,
            11,
            clothing(
                "Shirt",
                "ShirtTemplate",
                "http://www.roblox.com/asset/?id=77 ",
            ),
        ),
        worn_asset(
            3,
            12,
            clothing(
                "Pants",
                "PantsTemplate",
                "https://assetdelivery.roblox.com/v1/asset/?id=88",
            ),
        ),
        worn_asset(4, 2, clothing("ShirtGraphic", "Graphic", "rbxassetid://99")),
    ];
    assert_eq!(used(&dress(&mut dom, rig, &worn)), 3);
    let value = |class: &str, key: &str| {
        let found = children_of(&dom, rig, class);
        assert_eq!(found.len(), 1, "{class}");
        dom.get(found[0]).unwrap().properties().get(key).cloned()
    };
    assert_eq!(
        value("Shirt", "ShirtTemplate"),
        Some(uri("rbxassetid://77"))
    );
    assert_eq!(
        value("Pants", "PantsTemplate"),
        Some(uri("rbxassetid://88"))
    );
    assert_eq!(
        value("ShirtGraphic", "Graphic"),
        Some(uri("rbxassetid://99"))
    );
}

#[test]
fn clothing_with_no_texture_is_named_not_applied() {
    let avatar = avatar_r15();
    let (mut dom, rig, _) = rig_of(&avatar);
    let worn = vec![worn_asset(2, 11, clothing("Shirt", "ShirtTemplate", ""))];
    let fates = dress(&mut dom, rig, &worn);
    assert!(
        matches!(&fates[0], Fate::Left(why) if why.contains("empty texture")),
        "{fates:?}"
    );
    assert!(children_of(&dom, rig, "Shirt").is_empty());
}

#[test]
fn the_mock_serves_pants_and_a_tshirt_fixture() {
    let extra = r#",{"id":4,"name":"Jeans","assetType":{"id":12,"name":"Pants"}},{"id":5,"name":"Logo","assetType":{"id":2,"name":"TShirt"}}"#;
    let dir = mock_dir("clothes", &avatar_json("R15", extra));
    for (id, dom) in [
        (4, clothing("Pants", "PantsTemplate", "rbxassetid://88")),
        (5, clothing("ShirtGraphic", "Graphic", "rbxassetid://99")),
    ] {
        std::fs::write(
            dir.join(format!("{id}.rbxm")),
            rbx_binary::serialize(&dom).unwrap(),
        )
        .unwrap();
    }
    let fetched = mock_fetch(dir.to_str().unwrap(), Some(156)).unwrap();
    assert_eq!(fetched.worn.len(), 5);
    assert!(fetched.worn.iter().all(|w| w.dom.is_ok()));
    let (mut dom, rig, _) = rig_of(&fetched.avatar);
    // The Torso fixture is an empty model, so it is the one left out.
    assert_eq!(used(&dress(&mut dom, rig, &fetched.worn)), 4);
    assert_eq!(children_of(&dom, rig, "Pants").len(), 1);
    assert_eq!(children_of(&dom, rig, "ShirtGraphic").len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_download_says_whether_a_key_was_tried() {
    let refused = CloudError::AuthRequired { asset_id: 2 };
    assert!(download_error(&refused, false, None).contains("no Open Cloud key is stored"));
    let denied = CloudError::Http {
        status: 403,
        url: "x".into(),
    };
    assert!(download_error(&denied, true, None).contains("stored Open Cloud key"));
}

fn keyed(why: KeyedRefusal) -> CloudError {
    CloudError::KeyedAssetRefused {
        asset_id: 3,
        status: 403,
        why,
        detail: "detail".into(),
    }
}

#[test]
fn a_key_without_the_scope_is_told_which_one_and_where() {
    let said = download_error(&keyed(KeyedRefusal::NoAccess), true, Some(&Grant::Missing));
    assert!(
        said.contains("lacks the legacy-asset:manage permission"),
        "{said}"
    );
    assert!(
        said.contains("Creator Hub \u{203a} Open Cloud \u{203a} API Keys"),
        "{said}"
    );
    let by_body = download_error(&keyed(KeyedRefusal::Scope), true, None);
    assert!(
        by_body.contains("lacks the legacy-asset:manage"),
        "{by_body}"
    );
}

#[test]
fn a_key_with_the_scope_is_not_blamed_for_an_asset_it_cannot_read() {
    let said = download_error(
        &keyed(KeyedRefusal::NoAccess),
        true,
        Some(&Grant::Everywhere),
    );
    assert!(said.contains("own or created"), "{said}");
    assert!(!said.contains("lacks"), "{said}");
    let limited = download_error(
        &keyed(KeyedRefusal::NoAccess),
        true,
        Some(&Grant::Universes(vec![1])),
    );
    assert!(limited.contains("specific experiences"), "{limited}");
}

#[test]
fn a_rejected_key_is_not_a_missing_scope() {
    let said = download_error(
        &keyed(KeyedRefusal::InvalidKey),
        true,
        Some(&Grant::Everywhere),
    );
    assert!(said.contains("rejected the stored key"), "{said}");
}

fn layered_jacket(with_attachment: bool) -> WeakDom {
    let mut dom = WeakDom::new();
    let accessory = dom.new_instance("Accessory", "Jacket", None);
    let handle = dom.new_instance("MeshPart", "Handle", Some(accessory));
    if with_attachment {
        attach(&mut dom, handle, "BodyFrontAttachment", [0.; 3]);
    }
    let layer = dom.new_instance("WrapLayer", "WrapLayer", Some(handle));
    set(
        &mut dom,
        layer,
        "ReferenceOrigin",
        Variant::CFrame(Cf::at([0., 1., 0.]).data()),
    );
    dom
}

#[test]
fn layered_clothing_is_seated_on_its_body_part() {
    let avatar = avatar_r15();
    let (mut dom, rig, _) = rig_of(&avatar);
    let fates = dress(&mut dom, rig, &[worn_asset(9, 68, layered_jacket(false))]);
    let [Fate::Approximated(note)] = &fates[..] else {
        panic!("{fates:?}");
    };
    assert!(note.contains("nearest cage vertices"));
    let handle = child_named(&dom, child_named(&dom, rig, "Jacket").unwrap(), "Handle").unwrap();
    let torso = child_named(&dom, rig, "UpperTorso").unwrap();
    let position = |node| {
        let Some(Variant::CFrame(c)) = dom.get(node).unwrap().properties().get("CFrame") else {
            panic!("no CFrame");
        };
        (c.position.x, c.position.y, c.position.z)
    };
    assert_eq!(position(handle), position(torso));
}

#[test]
fn layered_clothing_is_refused_on_r6_and_approximations_are_noted() {
    let avatar = Avatar::from_json(&avatar_json("R6", "")).unwrap();
    let (mut dom, rig, _) = rig_of(&avatar);
    let fates = dress(&mut dom, rig, &[worn_asset(9, 68, layered_jacket(false))]);
    assert!(matches!(&fates[..], [Fate::Left(why)] if why.contains("R15")));

    let extra = r#",{"id":9,"name":"Jacket","assetType":{"id":68,"name":"SweaterAccessory"}}"#;
    let avatar = Avatar::from_json(&avatar_json("R15", extra)).unwrap();
    let mut worn = worn_of(&avatar);
    worn.push(worn_asset(3, 27, WeakDom::new()));
    worn.push(worn_asset(9, 68, layered_jacket(false)));
    let notes = notes_of(&avatar, &worn);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("Jacket") && n.contains("nearest cage vertices")),
        "{notes:?}"
    );
}

#[test]
fn makeup_is_grafted_onto_the_r15_head_and_refused_on_r6() {
    let makeup = || {
        let mut dom = WeakDom::new();
        let decal = dom.new_instance("Decal", "Lipstick", None);
        dom.new_instance("WrapTextureTransfer", "WrapTextureTransfer", Some(decal));
        dom
    };
    let (mut dom, rig, _) = rig_of(&avatar_r15());
    let fates = dress(&mut dom, rig, &[worn_asset(5, 89, makeup())]);
    assert_eq!(fates, vec![Fate::Used]);
    let head = child_named(&dom, rig, "Head").unwrap();
    let decal = child_named(&dom, head, "Lipstick").unwrap();
    assert_eq!(children_of(&dom, decal, "WrapTextureTransfer").len(), 1);

    let r6 = Avatar::from_json(&avatar_json("R6", "")).unwrap();
    let (mut dom, rig, _) = rig_of(&r6);
    let fates = dress(&mut dom, rig, &[worn_asset(5, 89, makeup())]);
    assert!(matches!(&fates[..], [Fate::Left(_)]));
}

const DYNAMIC_EXTRA: &str = r#",{"id":12,"name":"Stevie - Head","assetType":{"id":79,"name":"DynamicHead"}},{"id":13,"name":"Mood","assetType":{"id":78,"name":"MoodAnimation"}},{"id":14,"name":"Glasses","assetType":{"id":42,"name":"FaceAccessory"}}"#;

fn dynamic_head_source() -> WeakDom {
    let mut dom = WeakDom::new();
    let mesh = dom.new_instance("SpecialMesh", "Mesh", None);
    set(&mut dom, mesh, "MeshId", uri("rbxassetid://888"));
    set(&mut dom, mesh, "TextureId", uri("rbxassetid://999"));
    dom
}

fn glasses() -> WeakDom {
    let mut dom = WeakDom::new();
    let acc = dom.new_instance("Accessory", "Glasses", None);
    let handle = dom.new_instance("Part", "Handle", Some(acc));
    attach(&mut dom, handle, "FaceFrontAttachment", [0.; 3]);
    dom
}

#[test]
fn a_dynamic_head_keeps_a_neutral_face_controls_takes_face_accessories_and_records_the_mood() {
    let avatar = Avatar::from_json(&avatar_json("R15", DYNAMIC_EXTRA)).unwrap();
    let worn = [
        worn_asset(12, DYNAMIC_HEAD, dynamic_head_source()),
        worn_asset(14, 42, glasses()),
    ];
    let mut options = options_for(&avatar, JointStyle::AnimationConstraint, [0.; 3], None);
    let early = apply_packages(&mut options, &worn);
    let mut dom = WeakDom::new();
    let root = dom.new_instance("DataModel", "Game", None);
    let rig = build_rig(&mut dom, &options, root);
    let late = dress(&mut dom, rig, &worn);
    assert!(merge(early, late).iter().all(|f| *f == Fate::Used));
    describe_avatar(&mut dom, rig, &avatar);
    let head = child_named(&dom, rig, "Head").unwrap();
    assert_eq!(children_of(&dom, head, "FaceControls").len(), 1);
    assert!(child_of_class(&dom, head, "Decal").is_none());
    let controls = child_of_class(&dom, head, "FaceControls").unwrap();
    let mood = child_named(&dom, controls, "MoodAnimationId").unwrap();
    assert_eq!(
        dom.get(mood).unwrap().properties().get("Value"),
        Some(&Variant::String("rbxassetid://13".into()))
    );
    let handle = child_named(&dom, child_named(&dom, rig, "Glasses").unwrap(), "Handle").unwrap();
    let weld = child_named(&dom, handle, "AccessoryWeld").unwrap();
    assert_eq!(
        dom.get(weld).unwrap().properties().get("Part1"),
        Some(&Variant::Ref(head))
    );
}

#[test]
fn a_dynamic_head_the_mock_refuses_falls_back_to_the_stock_head_with_a_note() {
    let dir = mock_dir("head-401", &avatar_json("R15", DYNAMIC_EXTRA));
    std::fs::write(dir.join("12.status"), "401").unwrap();
    let fetched = mock_fetch(dir.to_str().unwrap(), Some(156)).unwrap();
    let mut options = options_for(
        &fetched.avatar,
        JointStyle::AnimationConstraint,
        [0.; 3],
        None,
    );
    let early = apply_packages(&mut options, &fetched.worn);
    let mut dom = WeakDom::new();
    let root = dom.new_instance("DataModel", "Game", None);
    let rig = build_rig(&mut dom, &options, root);
    let late = dress(&mut dom, rig, &fetched.worn);
    let (_, notes) = settle(&fetched.avatar, &fetched.worn, &merge(early, late));
    let note = notes
        .iter()
        .find(|n| n.starts_with("Stevie - Head"))
        .unwrap();
    assert!(
        note.contains("HTTP 401") && note.contains("stock head"),
        "{note}"
    );
    let head = child_named(&dom, rig, "Head").unwrap();
    assert!(child_of_class(&dom, head, "FaceControls").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
