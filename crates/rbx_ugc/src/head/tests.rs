use super::landmarks::{LEFT_EYE, MOUTH, RIGHT_EYE};
use super::*;
use crate::Rule;

/// A 1 × 1.2 face plane at z = -0.5, two triangles.
fn plane() -> Geometry {
    Geometry {
        positions: vec![
            [-0.5, -0.6, -0.5],
            [0.5, -0.6, -0.5],
            [0.5, 0.6, -0.5],
            [-0.5, 0.6, -0.5],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        uvs: Vec::new(),
        influences: Vec::new(),
        bones: Vec::new(),
        in_studs: true,
    }
}

/// A cage vertex for every landmark UV, just in front of the plane at
/// `gap`, placed where its UV says on the face, padded with the rest of
/// the template's unique UVs (off the face, so no landmark matches them).
fn cage(gap: f32) -> Geometry {
    let mut uvs: Vec<[f32; 2]> = LEFT_EYE
        .iter()
        .chain(&RIGHT_EYE)
        .chain(&MOUTH)
        .copied()
        .collect();
    let landmarks = uvs.len();
    uvs.extend((landmarks..rules::TEMPLATE_UVS).map(|i| [0.001 * i as f32, 0.9]));
    Geometry {
        positions: uvs
            .iter()
            .map(|uv| [(3.5 - uv[0]) * 2.0, (0.5 - uv[1]) * 2.0, -0.5 - gap])
            .collect(),
        uvs,
        ..Geometry::default()
    }
}

fn shifted(by: f32) -> Vec<[f32; 3]> {
    plane()
        .positions
        .iter()
        .map(|p| [p[0], p[1] + by, p[2]])
        .collect()
}

/// A head that passes: all 17 poses active, every action moves the face.
fn head() -> Head {
    let controls: Vec<String> = REQUIRED_POSES.iter().map(|p| p.to_string()).collect();
    let poses = pose_list(&controls)
        .into_iter()
        .map(|(name, _)| (name, shifted(0.05)))
        .collect();
    Head {
        name: "Head".into(),
        face_controls: true,
        mesh: Some(plane()),
        facs: Some(Facs {
            active: controls.clone(),
            controls,
            poses,
        }),
        cage: Some(cage(0.01)),
        material: "Plastic".into(),
        transparency: 0.0,
        vertex_color: [1.0; 3],
        extraneous: Vec::new(),
        collision_fidelity: "Default".into(),
        scale_type: true,
    }
}

fn rules(head: &Head) -> Vec<(HeadRule, String)> {
    check(head)
        .into_iter()
        .map(|f| match f.rule {
            Rule::Head(rule) => (rule, f.message),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn a_complete_head_passes() {
    assert_eq!(rules(&head()), []);
}

#[test]
fn pose_list_has_the_five_actions_then_each_control() {
    let list = pose_list(&["JawDrop".to_string()]);
    assert_eq!(list.len(), 6);
    assert_eq!(list[4].0, "Sad expression");
    assert!(list[4].1.contains(&("LeftCheekRaiser".to_string(), 0.85)));
    assert_eq!(
        list[5],
        ("JawDrop".to_string(), vec![("JawDrop".to_string(), 1.0)])
    );
}

#[test]
fn no_face_controls_or_no_facs_is_reported() {
    let mut h = head();
    h.face_controls = false;
    h.facs = None;
    let found = rules(&h);
    assert_eq!(found[0].0, HeadRule::FaceControls);
    assert_eq!(found[1].0, HeadRule::FacsPoses);
    assert!(found[1].1.contains("no FACS data"));
}

#[test]
fn a_missing_and_an_inactive_pose_are_named() {
    let mut h = head();
    let facs = h.facs.as_mut().unwrap();
    facs.controls.retain(|c| c != "Pucker");
    facs.active.retain(|c| c != "JawDrop");
    let found = check(&h);
    assert_eq!(found.len(), 2);
    assert!(found[0].message.contains("missing FACS poses: Pucker"));
    assert_eq!(found[0].measured, Some(Value::Count(16)));
    assert!(found[1].message.contains("move nothing: JawDrop"));
}

#[test]
fn landmarks_need_the_cage_its_template_uvs_and_a_close_mesh() {
    let mut h = head();
    h.cage = None;
    assert!(rules(&h)[0].1.contains("no outer cage"));

    let mut h = head();
    h.cage.as_mut().unwrap().uvs[0] = [0.0, 0.0];
    let found = rules(&h);
    assert_eq!(found.len(), 1);
    assert!(found[0].1.contains("no left eye landmark"), "{found:?}");

    let mut h = head();
    h.cage = Some(cage(0.5));
    let found = rules(&h);
    assert_eq!(found.len(), 3);
    assert!(found
        .iter()
        .all(|(_, m)| m.contains("can\u{2019}t be projected")));
}

#[test]
fn an_action_that_leaves_its_landmark_alone_fails_alone() {
    let mut h = head();
    let facs = h.facs.as_mut().unwrap();
    facs.poses
        .iter_mut()
        .find(|(n, _)| n == "Mouth opens")
        .unwrap()
        .1 = shifted(0.001);
    let found = rules(&h);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, HeadRule::FacialAction);
    assert!(found[0].1.starts_with("Mouth opens: the mouth landmark"));
}

#[test]
fn a_pose_past_the_head_box_is_reported_with_its_size() {
    let mut h = head();
    h.facs.as_mut().unwrap().poses[5].1[2] = [0.5, 2.0, -0.5];
    let found = check(&h);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].rule, Rule::Head(HeadRule::EmoteBounds));
    assert!(found[0].message.contains("LeftEyeClosed"));
    let Some(Value::Studs(size)) = found[0].measured else {
        panic!("{:?}", found[0].measured)
    };
    assert!((size[1] - 2.55).abs() < 1e-5, "{size:?}");
}

#[test]
fn marketplace_part_settings_and_extra_objects() {
    let mut h = head();
    h.material = "Neon".into();
    h.transparency = 0.5;
    h.vertex_color = [1.0, 0.0, 1.0];
    h.extraneous = vec!["Script".into()];
    let found = rules(&h);
    assert_eq!(found.len(), 4);
    assert!(found[0].1.starts_with("Material is Neon"));
    assert_eq!(found[3].0, HeadRule::Extraneous);
}

#[test]
fn studio_schema_and_the_cage_template_uv_count() {
    let mut h = head();
    h.scale_type = false;
    h.collision_fidelity = "Hull".into();
    if let Some(cage) = &mut h.cage {
        cage.uvs.truncate(85);
        cage.positions.truncate(85);
    }
    let found = rules(&h);
    let kinds: Vec<HeadRule> = found.iter().map(|f| f.0).collect();
    assert_eq!(
        kinds,
        [HeadRule::Schema, HeadRule::Schema, HeadRule::CageTemplate],
        "{found:?}"
    );
    assert!(found[0].1.contains("AvatarPartScaleType"));
    assert!(found[1].1.starts_with("CollisionFidelity is Hull"));
    assert!(found[2].1.starts_with("The outer cage has 85 unique UVs"));
}

#[test]
fn every_rule_cites_a_dynamic_head_page() {
    for rule in [
        HeadRule::FaceControls,
        HeadRule::Landmarks,
        HeadRule::Schema,
        HeadRule::CageTemplate,
    ] {
        assert!(rule.source().contains("/avatar/dynamic-heads/"));
        assert!(!rule.title().is_empty());
    }
}
