use rbx_dom::merge::{Merge, Side, Target};
use rbx_dom::{Instance, Ref, UniqueId, Variant, WeakDom};

use super::{sidecar, target_label, MergeSession};

fn dom(first: u32, transparency: f32, reflectance: f32) -> WeakDom {
    let mut dom = WeakDom::new();
    let mut part = Instance::new(Ref::new(first), "Part", "Part");
    part.properties_mut().insert(
        "UniqueId".into(),
        Variant::UniqueId(UniqueId {
            index: 1,
            time: 1,
            random: 7,
        }),
    );
    part.properties_mut()
        .insert("Transparency".into(), Variant::Float32(transparency));
    part.properties_mut()
        .insert("Reflectance".into(), Variant::Float32(reflectance));
    dom.insert(part);
    dom
}

#[test]
fn conflicts_on_one_instance_group_into_one_node() {
    let merge = Merge::new(dom(1, 0.0, 0.0), dom(101, 0.5, 0.5), dom(201, 0.9, 0.9));
    let ids: Vec<usize> = merge.conflicts().iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), 2);
    let session = MergeSession::new(merge, None, "theirs.rbxl".into());
    assert_eq!(session.nodes.len(), 1);
    assert_eq!(session.nodes[0].properties.len(), 2);
    assert_eq!(session.nodes[0].path, "game");
    assert_eq!(session.by_node, vec![ids]);
}

#[test]
fn labels_cover_every_target() {
    assert_eq!(target_label(&Target::Name), "Name");
    assert_eq!(target_label(&Target::Class), "ClassName");
    assert_eq!(target_label(&Target::Property("Size".into())), "Size");
    assert_eq!(target_label(&Target::Attribute("A".into())), "Attribute A");
    assert_eq!(target_label(&Target::Source(0)), "Source hunk 1");
    assert_eq!(target_label(&Target::Parent), "Parent");
    assert_eq!(
        target_label(&Target::Existence {
            removed_by: Side::Ours
        }),
        "Removed by us"
    );
    assert_eq!(
        target_label(&Target::Existence {
            removed_by: Side::Theirs
        }),
        "Removed by them"
    );
}

fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("rbx-place-merge-{}-{name}", std::process::id()))
}

#[test]
fn sidecar_round_trips_and_rejects_bad_version() {
    use base64::Engine as _;
    let bytes = rbx_binary::serialize(&dom(1, 0.0, 0.0)).unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let path = temp("ok.merge.json");
    let json = serde_json::json!({
        "version": 1, "file": "a.rbxl", "base": encoded, "theirs": encoded,
    });
    std::fs::write(&path, json.to_string()).unwrap();
    let read = sidecar::read(&path).unwrap();
    assert_eq!(read.file, "a.rbxl");
    assert_eq!(read.base.root_refs().len(), 1);
    assert_eq!(read.theirs.root_refs().len(), 1);
    std::fs::remove_file(&path).unwrap();

    let bad = temp("bad.merge.json");
    std::fs::write(&bad, r#"{"version": 2}"#).unwrap();
    assert!(sidecar::read(&bad).is_err());
    std::fs::remove_file(&bad).unwrap();
}
