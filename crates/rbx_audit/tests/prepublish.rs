//! The pre-publish check over a generated fixture place. Script lines that
//! should be flagged end in `-- expect: <check-id>`; the place-level
//! findings (assets, parts, geometry) are listed in [`PLACE_EXPECTED`].
//! The same place is committed as `assets/tests/prepublish/PrePublish.rbxlx`
//! for `rbxaudit --prepublish` and the editor's dock (see
//! [`write_fixture_place`]).

use std::collections::{BTreeSet, HashMap};

use rbx_assets::AssetRef;
use rbx_audit::prepublish::{self, Inputs, Presence};
use rbx_audit::{Check, Finding, Severity};
use rbx_dom::{CFrameData, Content, Ref, Variant, Vector3Data, WeakDom};

const SCRIPTS: [(&str, &str, &str, &str); 4] = [
    (
        "ServerScriptService",
        "Script",
        "Deprecated",
        include_str!("../../../assets/tests/prepublish/Deprecated.server.luau"),
    ),
    (
        "ServerScriptService",
        "Script",
        "Shadowed",
        include_str!("../../../assets/tests/prepublish/Shadowed.server.luau"),
    ),
    (
        "ServerScriptService",
        "Script",
        "Data",
        include_str!("../../../assets/tests/prepublish/Data.server.luau"),
    ),
    (
        "StarterPlayer.StarterPlayerScripts",
        "LocalScript",
        "Streaming",
        include_str!("../../../assets/tests/prepublish/Streaming.client.luau"),
    ),
];

/// Findings about an instance rather than a line, with the presence map
/// [`presence`] gives.
const PLACE_EXPECTED: [(&str, Check, Severity); 12] = [
    (
        "Workspace.Props.BadMesh",
        Check::BrokenAsset,
        Severity::High,
    ),
    (
        "Workspace.Props.Holder.Gone",
        Check::BrokenAsset,
        Severity::High,
    ),
    (
        "Workspace.Props.Dense",
        Check::HeavyGeometry,
        Severity::High,
    ),
    (
        "Workspace.Props.EmptyMesh",
        Check::BrokenAsset,
        Severity::Medium,
    ),
    (
        "Workspace.Props.Holder.Private",
        Check::BrokenAsset,
        Severity::Medium,
    ),
    (
        "Workspace.Props.Holder.NoId",
        Check::BrokenAsset,
        Severity::Medium,
    ),
    (
        "Workspace.Props.BigUnion",
        Check::HeavyGeometry,
        Severity::Medium,
    ),
    ("Workspace.Crates", Check::UnanchoredParts, Severity::Medium),
    ("Workspace.Pair", Check::UnanchoredParts, Severity::Medium),
    ("Workspace", Check::UnanchoredParts, Severity::Medium),
    (
        "Workspace.Props.Holder.Placeholder",
        Check::BrokenAsset,
        Severity::Low,
    ),
    // The Security Audit's own findings come along.
    (
        "ReplicatedStorage.Orphan",
        Check::NoServerListener,
        Severity::Low,
    ),
];

const PREPUBLISH_CHECKS: [Check; 8] = [
    Check::BrokenAsset,
    Check::AssetsNotChecked,
    Check::DeprecatedApi,
    Check::UnanchoredParts,
    Check::DataStoreWithoutPcall,
    Check::HeavyGeometry,
    Check::PlaceBudget,
    Check::StreamingIndex,
];

fn path(dom: &mut WeakDom, path: &str) -> Ref {
    let mut parts = path.split('.');
    let service = parts.next().unwrap();
    let mut at = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).unwrap().name() == service)
        .unwrap_or_else(|| dom.new_instance(service, service, None));
    for part in parts {
        let existing = dom
            .get(at)
            .unwrap()
            .children()
            .iter()
            .copied()
            .find(|c| dom.get(*c).unwrap().name() == part);
        at = existing.unwrap_or_else(|| dom.new_instance(part, part, Some(at)));
    }
    at
}

fn v(x: f32, y: f32, z: f32) -> Vector3Data {
    Vector3Data { x, y, z }
}

/// A part at `(x, y, z)`, anchored or not.
fn part(
    dom: &mut WeakDom,
    class: &str,
    name: &str,
    parent: Ref,
    at: [f32; 3],
    anchored: bool,
) -> Ref {
    let r = dom.new_instance(class, name, Some(parent));
    let cframe = CFrameData {
        position: v(at[0], at[1], at[2]),
        rotation: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
    };
    let _ = dom.set_property(r, "CFrame", Variant::CFrame(cframe));
    let _ = dom.set_property(r, "size", Variant::Vector3(v(2., 2., 2.)));
    let _ = dom.set_property(r, "Anchored", Variant::Bool(anchored));
    r
}

fn set(dom: &mut WeakDom, r: Ref, key: &str, value: Variant) {
    let _ = dom.set_property(r, key, value);
}

fn uri(text: &str) -> Variant {
    Variant::Content(Content::Uri(text.into()))
}

pub fn fixture() -> WeakDom {
    let mut dom = WeakDom::new();
    for service in [
        "Workspace",
        "Players",
        "ServerScriptService",
        "ReplicatedStorage",
    ] {
        dom.new_instance(service, service, None);
    }
    let workspace = path(&mut dom, "Workspace");
    set(&mut dom, workspace, "StreamingEnabled", Variant::Bool(true));
    let player = path(&mut dom, "StarterPlayer");
    dom.new_instance("StarterPlayerScripts", "StarterPlayerScripts", Some(player));
    let storage = path(&mut dom, "ReplicatedStorage");
    dom.new_instance("RemoteEvent", "Orphan", Some(storage));
    for (parent, class, name, source) in SCRIPTS {
        let parent = path(&mut dom, parent);
        let script = dom.new_instance(class, name, Some(parent));
        set(&mut dom, script, "Source", Variant::String(source.into()));
    }

    let baseplate = part(
        &mut dom,
        "Part",
        "Baseplate",
        workspace,
        [0., -0.5, 0.],
        true,
    );
    set(
        &mut dom,
        baseplate,
        "size",
        Variant::Vector3(v(64., 1., 64.)),
    );
    let map = dom.new_instance("Folder", "Map", Some(workspace));
    part(&mut dom, "Part", "Door", map, [0., 3., -10.], true);
    part(&mut dom, "Part", "Lamp", workspace, [6., 3., -10.], true);

    // Assets, on anchored parts so only the asset rules speak.
    let props = dom.new_instance("Model", "Props", Some(workspace));
    let bad = part(&mut dom, "MeshPart", "BadMesh", props, [-8., 1., 8.], true);
    set(&mut dom, bad, "MeshId", uri("rbxassetid://abc"));
    let empty = part(
        &mut dom,
        "MeshPart",
        "EmptyMesh",
        props,
        [-4., 1., 8.],
        true,
    );
    set(&mut dom, empty, "MeshId", uri(""));
    let dense = part(&mut dom, "MeshPart", "Dense", props, [0., 1., 8.], true);
    set(&mut dom, dense, "MeshId", uri("rbxassetid://777"));
    // `TextureID` legacy-spelled as a plain string; that one is fine.
    set(
        &mut dom,
        dense,
        "TextureID",
        Variant::String("rbxassetid://1001".into()),
    );
    let holder = part(&mut dom, "Part", "Holder", props, [4., 1., 8.], true);
    let gone = dom.new_instance("Decal", "Gone", Some(holder));
    set(&mut dom, gone, "Texture", uri("rbxassetid://404"));
    let private = dom.new_instance("Sound", "Private", Some(holder));
    set(&mut dom, private, "SoundId", uri("rbxassetid://403"));
    let placeholder = dom.new_instance("Sound", "Placeholder", Some(holder));
    set(&mut dom, placeholder, "SoundId", uri(""));
    let anim = dom.new_instance("Animation", "Walk", Some(holder));
    set(&mut dom, anim, "AnimationId", uri("rbxassetid://1001"));
    let no_id = dom.new_instance("Animation", "NoId", Some(holder));
    set(&mut dom, no_id, "AnimationId", uri(""));
    let native = dom.new_instance("Decal", "Native", Some(holder));
    set(
        &mut dom,
        native,
        "Texture",
        uri("rbxasset://textures/SpawnLocation.png"),
    );
    let big = part(
        &mut dom,
        "UnionOperation",
        "BigUnion",
        props,
        [8., 1., 8.],
        true,
    );
    set(&mut dom, big, "TriangleCount", Variant::Int32(12_000));
    let small = part(
        &mut dom,
        "UnionOperation",
        "SmallUnion",
        props,
        [12., 1., 8.],
        true,
    );
    set(&mut dom, small, "TriangleCount", Variant::Int32(500));

    // Loose parts: two crates, a pair welded only to each other, one in
    // the open.
    let crates = dom.new_instance("Model", "Crates", Some(workspace));
    part(&mut dom, "Part", "Crate", crates, [-10., 4., -4.], false);
    part(&mut dom, "Part", "Lid", crates, [-10., 6., -4.], false);
    let pair = dom.new_instance("Model", "Pair", Some(workspace));
    let a = part(&mut dom, "Part", "A", pair, [-14., 4., -4.], false);
    let b = part(&mut dom, "Part", "B", pair, [-14., 6., -4.], false);
    let weld = dom.new_instance("WeldConstraint", "Weld", Some(a));
    set(&mut dom, weld, "Part0", Variant::Ref(a));
    set(&mut dom, weld, "Part1", Variant::Ref(b));
    part(&mut dom, "Part", "Loose", workspace, [-18., 4., -4.], false);

    // Held parts: welded to an anchored body, roped to an anchored beam,
    // floated by a mover, a character's, a tool's.
    let car = dom.new_instance("Model", "Car", Some(workspace));
    let body = part(&mut dom, "Part", "Body", car, [10., 2., -4.], true);
    let wheel = part(&mut dom, "Part", "Wheel", car, [10., 1., -6.], false);
    let motor = dom.new_instance("Motor6D", "Axle", Some(body));
    set(&mut dom, motor, "Part0", Variant::Ref(body));
    set(&mut dom, motor, "Part1", Variant::Ref(wheel));
    let beam = part(&mut dom, "Part", "Beam", workspace, [14., 10., -4.], true);
    let lamp = part(
        &mut dom,
        "Part",
        "Hanging",
        workspace,
        [14., 6., -4.],
        false,
    );
    let top = dom.new_instance("Attachment", "Top", Some(beam));
    let bottom = dom.new_instance("Attachment", "Bottom", Some(lamp));
    let rope = dom.new_instance("RopeConstraint", "Rope", Some(beam));
    set(&mut dom, rope, "Attachment0", Variant::Ref(top));
    set(&mut dom, rope, "Attachment1", Variant::Ref(bottom));
    let floating = part(
        &mut dom,
        "Part",
        "Floating",
        workspace,
        [18., 6., -4.],
        false,
    );
    let float_at = dom.new_instance("Attachment", "At", Some(floating));
    let align = dom.new_instance("AlignPosition", "Hold", Some(floating));
    set(&mut dom, align, "Attachment0", Variant::Ref(float_at));
    let npc = dom.new_instance("Model", "Npc", Some(workspace));
    dom.new_instance("Humanoid", "Humanoid", Some(npc));
    part(
        &mut dom,
        "Part",
        "HumanoidRootPart",
        npc,
        [22., 3., -4.],
        false,
    );
    let tool = dom.new_instance("Tool", "Sword", Some(workspace));
    part(&mut dom, "Part", "Handle", tool, [26., 1., -4.], false);
    dom
}

fn presence() -> HashMap<u64, Presence> {
    HashMap::from([
        (404, Presence::Missing),
        (403, Presence::Restricted),
        (1001, Presence::Exists),
        (777, Presence::Exists),
    ])
}

/// A `version 2.00` mesh of `triangles` faces over three vertices.
fn mesh(triangles: u32) -> Vec<u8> {
    let mut bytes = b"version 2.00\n".to_vec();
    bytes.extend(12u16.to_le_bytes());
    bytes.extend([40u8, 12]);
    bytes.extend(3u32.to_le_bytes());
    bytes.extend(triangles.to_le_bytes());
    bytes.extend([0u8; 120]);
    for _ in 0..triangles {
        for index in 0u32..3 {
            bytes.extend(index.to_le_bytes());
        }
    }
    bytes
}

fn run(dom: &WeakDom, presence: Option<&HashMap<u64, Presence>>) -> Vec<Finding> {
    let dense = mesh(20_001);
    let mut fetch = |asset: &AssetRef| (*asset == AssetRef::Id(777)).then(|| dense.clone());
    prepublish::check(
        dom,
        Inputs {
            presence,
            fetch: &mut fetch,
        },
    )
}

#[test]
fn the_fixture_place_is_flagged_exactly_where_marked() {
    let mut expected = BTreeSet::new();
    for (parent, _, name, source) in SCRIPTS {
        for (index, line) in source.lines().enumerate() {
            if let Some((_, id)) = line.split_once("-- expect: ") {
                expected.insert((
                    format!("{parent}.{name}"),
                    Some(index + 1),
                    id.trim().to_string(),
                ));
            }
        }
    }
    for (path, check, _) in PLACE_EXPECTED {
        expected.insert((path.to_string(), None, check.id().to_string()));
    }

    let presence = presence();
    let findings = run(&fixture(), Some(&presence));
    let found: BTreeSet<_> = findings
        .iter()
        .filter(|f| f.line.is_none() || PREPUBLISH_CHECKS.contains(&f.check))
        .map(|f| (f.path.clone(), f.line, f.check.id().to_string()))
        .collect();
    let false_positives: Vec<_> = found.difference(&expected).collect();
    let missed: Vec<_> = expected.difference(&found).collect();
    assert!(
        false_positives.is_empty(),
        "false positives: {false_positives:#?}"
    );
    assert!(missed.is_empty(), "missed: {missed:#?}");

    for (path, check, severity) in PLACE_EXPECTED {
        assert!(
            findings
                .iter()
                .any(|f| f.path == path && f.check == check && f.severity == severity),
            "{path} {} should be {}",
            check.id(),
            severity.label()
        );
    }
}

#[test]
fn findings_say_what_is_wrong() {
    let presence = presence();
    let findings = run(&fixture(), Some(&presence));
    let message = |path: &str, check: Check| {
        findings
            .iter()
            .find(|f| f.path == path && f.check == check)
            .map(|f| f.message.clone())
            .unwrap()
    };
    assert!(message("Workspace.Crates", Check::UnanchoredParts).starts_with("2 unanchored"));
    assert!(message("Workspace.Pair", Check::UnanchoredParts).starts_with("2 unanchored"));
    assert!(message("Workspace", Check::UnanchoredParts).contains("`Loose`"));
    assert!(message("Workspace.Props.Holder.Gone", Check::BrokenAsset).contains("asset 404"));
    assert!(message("Workspace.Props.Dense", Check::HeavyGeometry).contains("20001 triangles"));
    assert!(message("Workspace.Props.BigUnion", Check::HeavyGeometry).contains("12000"));
}

#[test]
fn offline_the_ids_are_reported_not_checked() {
    let findings = run(&fixture(), None);
    let unchecked: Vec<_> = findings
        .iter()
        .filter(|f| f.check == Check::AssetsNotChecked)
        .collect();
    assert_eq!(unchecked.len(), 1);
    assert_eq!(unchecked[0].path, "Workspace");
    assert!(unchecked[0]
        .message
        .starts_with("4 asset id(s) not checked"));
    // Malformed and empty references don't need the network.
    assert!(findings
        .iter()
        .any(|f| f.path == "Workspace.Props.BadMesh" && f.check == Check::BrokenAsset));
    assert!(!findings
        .iter()
        .any(|f| f.path == "Workspace.Props.Holder.Gone" && f.check == Check::BrokenAsset));
}

#[test]
fn the_asset_ids_to_ask_about_are_each_listed_once() {
    assert_eq!(prepublish::asset_ids(&fixture()), [403, 404, 777, 1001]);
}

#[test]
fn a_mesh_under_the_limit_or_not_cached_is_not_flagged() {
    let presence = presence();
    let small = mesh(100);
    for bytes in [Some(small), None] {
        let mut fetch = |_: &AssetRef| bytes.clone();
        let findings = prepublish::check(
            &fixture(),
            Inputs {
                presence: Some(&presence),
                fetch: &mut fetch,
            },
        );
        assert!(!findings
            .iter()
            .any(|f| f.path == "Workspace.Props.Dense" && f.check == Check::HeavyGeometry));
    }
}

#[test]
fn budgets_flag_only_past_the_threshold() {
    let budget = |parts: usize| {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        for i in 0..parts {
            part(
                &mut dom,
                "Part",
                &format!("P{i}"),
                workspace,
                [0., 0., 0.],
                true,
            );
        }
        let mut fetch = |_: &AssetRef| None;
        prepublish::check(
            &dom,
            Inputs {
                presence: None,
                fetch: &mut fetch,
            },
        )
        .into_iter()
        .filter(|f| f.check == Check::PlaceBudget)
        .count()
    };
    assert_eq!(budget(prepublish::PART_BUDGET), 0);
    assert_eq!(budget(prepublish::PART_BUDGET + 1), 1);
}

#[test]
fn the_slim_copy_checks_the_same() {
    let presence = presence();
    let dom = fixture();
    assert_eq!(
        run(&dom, Some(&presence)),
        run(&prepublish::slim_copy(&dom), Some(&presence))
    );
}

/// `RBX_PREPUBLISH_FIXTURE_OUT=assets/tests/prepublish/PrePublish.rbxlx
/// cargo test -p rbx_audit --test prepublish -- --ignored` regenerates the
/// committed place.
#[test]
#[ignore]
fn write_fixture_place() {
    let out = std::env::var("RBX_PREPUBLISH_FIXTURE_OUT").expect("RBX_PREPUBLISH_FIXTURE_OUT");
    std::fs::write(out, rbx_xml::serialize(&fixture()).unwrap()).unwrap();
}
