//! Property decoders checked against the real bytes of both fixture files.
//!
//! Every expectation here is a value a human can recognize in Roblox Studio, which
//! is what rules out a decoder that merely produces well-formed garbage.

mod support;

use std::collections::BTreeSet;

use rbx_binary::deserialize;
use rbx_dom::{Content, FontStyle, Variant};

use support::{determinant, every, instances_of, only, property, FPS, TEST_PLACE};

#[test]
fn spawn_location_team_color_is_medium_stone_grey() {
    let dom = deserialize(TEST_PLACE).unwrap();
    let spawn = only(&dom, "SpawnLocation");

    // 194 is "Medium stone grey", the TeamColor of an untouched SpawnLocation.
    assert_eq!(property(spawn, "TeamColor"), &Variant::BrickColor(194));
}

#[test]
fn ui_gradient_transparency_is_a_flat_two_keypoint_sequence() {
    let dom = deserialize(TEST_PLACE).unwrap();
    let gradient = only(&dom, "UIGradient");

    let Variant::NumberSequence(sequence) = property(gradient, "Transparency") else {
        panic!("Transparency is not a NumberSequence");
    };

    let points: Vec<(f32, f32, f32)> = sequence
        .keypoints
        .iter()
        .map(|k| (k.time, k.value, k.envelope))
        .collect();
    assert_eq!(points, vec![(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)]);
}

#[test]
fn ui_gradient_color_is_white_at_both_ends() {
    let dom = deserialize(TEST_PLACE).unwrap();
    let gradient = only(&dom, "UIGradient");

    let Variant::ColorSequence(sequence) = property(gradient, "Color") else {
        panic!("Color is not a ColorSequence");
    };

    assert_eq!(sequence.keypoints.len(), 2);
    assert_eq!(sequence.keypoints[0].time, 0.0);
    assert_eq!(sequence.keypoints[1].time, 1.0);
    for keypoint in &sequence.keypoints {
        assert_eq!(
            (keypoint.color.r, keypoint.color.g, keypoint.color.b),
            (1.0, 1.0, 1.0)
        );
    }
}

// StarterPlayer and AvatarBodyRules describe the same avatar scale limits, so the
// two properties must decode to the same pair of bounds.
#[test]
fn avatar_scale_ranges_are_the_studio_defaults() {
    let dom = deserialize(TEST_PLACE).unwrap();
    let starter = only(&dom, "StarterPlayer");
    let rules = only(&dom, "AvatarBodyRules");

    let range = |value: &Variant| match value {
        Variant::NumberRange(range) => (range.min, range.max),
        other => panic!("expected a NumberRange, got {other:?}"),
    };

    assert_eq!(
        range(property(starter, "GameSettingsScaleRangeHeight")),
        (0.9, 1.05)
    );
    assert_eq!(
        range(property(starter, "GameSettingsScaleRangeWidth")),
        (0.7, 1.0)
    );
    assert_eq!(
        range(property(starter, "GameSettingsScaleRangeHead")),
        (0.95, 1.0)
    );

    assert_eq!(range(property(rules, "CustomHeightScale")), (0.9, 1.05));
    assert_eq!(range(property(rules, "CustomWidthScale")), (0.7, 1.0));
    assert_eq!(range(property(rules, "CustomHeight")), (5.5, 5.5));
}

#[test]
fn image_label_slice_center_is_the_empty_rect() {
    let dom = deserialize(TEST_PLACE).unwrap();
    let label = only(&dom, "ImageLabel");

    let Variant::Rect(rect) = property(label, "SliceCenter") else {
        panic!("SliceCenter is not a Rect");
    };

    assert_eq!((rect.min.x, rect.min.y), (0.0, 0.0));
    assert_eq!((rect.max.x, rect.max.y), (0.0, 0.0));
}

// FPS.rbxm carries all three shapes of the type: a present value with a raw
// matrix (Model), and two absent values sharing one array (the two Tools).
#[test]
fn fps_world_pivots_hold_one_real_matrix_and_two_absences() {
    let dom = deserialize(FPS).unwrap();

    let Variant::OptionalCFrame(Some(pivot)) = property(only(&dom, "Model"), "WorldPivotData")
    else {
        panic!("the Model's WorldPivotData should be present");
    };

    let det = determinant(&pivot.rotation);
    assert!(
        (det - 1.0).abs() < 1e-4,
        "the pivot rotation is not orthonormal: {det}"
    );
    // The pivot sits on the model it belongs to, a knife held a few studs up.
    assert!(
        (pivot.position.y - 5.19).abs() < 0.01,
        "unexpected pivot height {}",
        pivot.position.y
    );

    let tools = instances_of(&dom, "Tool");
    assert_eq!(tools.len(), 2);
    for tool in tools {
        assert_eq!(
            property(tool, "WorldPivotData"),
            &Variant::OptionalCFrame(None)
        );
    }
}

// Workspace's pivot is the shortest possible payload of the type: 16 bytes for one
// present value with a compressed identity rotation at the origin.
#[test]
fn test_place_workspace_pivot_is_the_identity_at_the_origin() {
    let dom = deserialize(TEST_PLACE).unwrap();

    let Variant::OptionalCFrame(Some(pivot)) = property(only(&dom, "Workspace"), "WorldPivotData")
    else {
        panic!("the Workspace's WorldPivotData should be present");
    };

    assert_eq!(
        (pivot.position.x, pivot.position.y, pivot.position.z),
        (0.0, 0.0, 0.0)
    );
    assert_eq!(
        pivot.rotation,
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    );
}

// Instances created together share `time` and `random`, and their `index` is a
// counter, so a whole file's worth of identifiers cross-checks every field of the
// struct at once: a wrong field offset would scatter all three.
#[test]
fn test_place_unique_ids_come_from_two_save_sessions() {
    let dom = deserialize(TEST_PLACE).unwrap();

    let ids: Vec<_> = every(&dom, "UniqueId")
        .into_iter()
        .map(|value| match value {
            Variant::UniqueId(id) => id,
            other => panic!("expected a UniqueId, got {other:?}"),
        })
        .collect();
    assert_eq!(ids.len(), 81);

    // No two instances share an identifier — but the index alone is not it: Sky
    // and Packages both got 0x3ae, in two different sessions.
    let unique: BTreeSet<(u32, u32, i64)> = ids
        .iter()
        .map(|id| (id.index, id.time, id.random))
        .collect();
    assert_eq!(unique.len(), ids.len());
    assert_eq!(
        ids.iter()
            .map(|id| id.index)
            .collect::<BTreeSet<u32>>()
            .len(),
        80
    );

    // 0x09e550b2 is 2026-04-06 and 0x0ab73b17 is 2026-09-12, counted in seconds
    // from 2021-01-01: the day the place was created and the day it was edited.
    const CREATED: u32 = 0x09e5_5048;
    const EDITED: u32 = 0x0ab7_3b17;
    for id in &ids {
        assert!(
            (CREATED..=EDITED).contains(&id.time),
            "{:#x} is not inside the file's editing window",
            id.time
        );
        // The rotation is what keeps this positive; reading the field raw would
        // make the values written in the later session negative.
        assert!(id.random > 0, "random {:#x} decoded as negative", id.random);
    }

    // Instances created together share one random, so the file must not show 81.
    let randoms: BTreeSet<i64> = ids.iter().map(|id| id.random).collect();
    assert!(
        (2..=8).contains(&randoms.len()),
        "expected a handful of save sessions, got {}",
        randoms.len()
    );
}

// HistoryId is written next to UniqueId on every instance but is always blank,
// which makes it the control case: an all-zero struct must stay all-zero.
#[test]
fn test_place_history_ids_are_all_blank() {
    let dom = deserialize(TEST_PLACE).unwrap();

    let history = every(&dom, "HistoryId");
    assert_eq!(history.len(), 81);
    for value in history {
        let Variant::UniqueId(id) = value else {
            panic!("HistoryId is not a UniqueId");
        };
        assert_eq!((id.index, id.time, id.random), (0, 0, 0));
    }
}

#[test]
fn chat_configuration_font_faces_are_builder_sans() {
    let dom = deserialize(TEST_PLACE).unwrap();

    let faces: Vec<_> = every(&dom, "FontFace")
        .into_iter()
        .map(|value| match value {
            Variant::Font(font) => font,
            other => panic!("expected a Font, got {other:?}"),
        })
        .collect();
    assert_eq!(faces.len(), 4);

    for face in &faces {
        assert_eq!(face.family, "rbxasset://fonts/families/BuilderSans.json");
        assert_eq!(face.style, FontStyle::Normal);
        // The weight is the CSS-style number itself, not an enum ordinal.
        assert!(
            [500, 700].contains(&face.weight),
            "unexpected weight {}",
            face.weight
        );
        // The cached face must agree with the weight, which is what proves the
        // u16 and the trailing string were not read from the wrong offsets.
        let expected = if face.weight == 700 { "Bold" } else { "Medium" };
        let cached = face.cached_face_id.as_deref().unwrap_or_default();
        assert!(
            cached.contains(expected),
            "weight {} cached as {cached}",
            face.weight
        );
    }

    assert_eq!(faces.iter().filter(|face| face.weight == 700).count(), 1);
}

// Capabilities appears on every instance of both files and is always empty; the
// point of the check is that it no longer decodes as a plain Int64.
#[test]
fn capabilities_decode_to_an_empty_bitfield_everywhere() {
    for bytes in [FPS, TEST_PLACE] {
        let dom = deserialize(bytes).unwrap();
        let values = every(&dom, "Capabilities");

        assert!(!values.is_empty());
        for value in values {
            assert_eq!(value, Variant::SecurityCapabilities(0));
        }
    }
}

#[test]
fn emissive_mask_content_is_empty_on_both_decals() {
    let dom = deserialize(TEST_PLACE).unwrap();

    let values = every(&dom, "EmissiveMaskContent");
    assert_eq!(values.len(), 2);
    for value in values {
        assert_eq!(value, Variant::Content(Content::None));
    }
}

// The whole point of the exercise: nothing in either file falls back to `Unknown`
// any more except the two String properties that hold non-UTF-8 blobs.
#[test]
fn only_non_utf8_string_blobs_still_degrade_to_unknown() {
    let mut unknown = Vec::new();
    for bytes in [FPS, TEST_PLACE] {
        let dom = deserialize(bytes).unwrap();
        for referent in support::all_refs(&dom) {
            let Some(instance) = dom.get(referent) else {
                continue;
            };
            for (name, value) in instance.properties() {
                if let Variant::Unknown { type_id, .. } = value {
                    unknown.push((name.clone(), *type_id));
                }
            }
        }
    }
    unknown.sort();

    assert_eq!(
        unknown,
        vec![
            ("CollisionGroupData".to_owned(), 0x01),
            ("MaterialColors".to_owned(), 0x01),
        ]
    );
}
