//! The graph editor's layout for one script, remembered on this machine
//! only: node positions, groups and the camera, keyed by each node's
//! codegen anchor so they survive edits that renumber the graph.
//! Files live under `<config dir>/script-layouts/<hash>.json`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use rbx_dom::{Ref, WeakDom};
use serde::{Deserialize, Serialize};

use super::{Graph, Group, NodeId};
use crate::settings::{default_config_dir, write_atomic};

const VERSION: u32 = 1;

/// Which script of which place a layout belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LayoutKey {
    /// The place file's path, or `cloud:<placeId>`.
    pub(crate) place: String,
    /// The script's DataModel path, or `ref:<referent>` when it has none.
    pub(crate) script: String,
}

/// `path` is the open place file; `cloud_place_id` wins when the place was
/// opened from Roblox rather than a file.
pub(crate) fn key_for(
    path: &Path,
    cloud_place_id: Option<u64>,
    dom: &WeakDom,
    script: Ref,
) -> LayoutKey {
    let place = match cloud_place_id {
        Some(id) => format!("cloud:{id}"),
        None => std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned(),
    };
    let mut names = Vec::new();
    let mut at = Some(script);
    while let Some(reference) = at {
        let Some(instance) = dom.get(reference) else {
            break;
        };
        names.push(instance.name().to_owned());
        at = dom.parent(reference);
    }
    names.reverse();
    let script = if names.is_empty() {
        format!("ref:{script:?}")
    } else {
        names.join(".")
    };
    LayoutKey { place, script }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct View {
    pub(crate) zoom: f32,
    pub(crate) pan_x: f32,
    pub(crate) pan_y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedGroup {
    pub(crate) title: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    /// Anchors of the nodes that sat inside the frame.
    pub(crate) members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Layout {
    version: u32,
    pub(crate) nodes: BTreeMap<String, (f32, f32)>,
    pub(crate) groups: Vec<SavedGroup>,
    pub(crate) view: Option<View>,
}

/// FNV-1a over `place NUL script`: stable across runs and builds, unlike
/// `DefaultHasher`.
fn file_name(key: &LayoutKey) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key
        .place
        .bytes()
        .chain(std::iter::once(0))
        .chain(key.script.bytes())
    {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}.json")
}

fn root() -> Option<PathBuf> {
    default_config_dir().map(|d| d.join("script-layouts"))
}

pub(crate) fn load(key: &LayoutKey) -> Option<Layout> {
    load_from(&root()?, key)
}

pub(crate) fn save(key: &LayoutKey, layout: &Layout) {
    if let Some(dir) = root() {
        save_in(&dir, key, layout);
    }
}

fn load_from(dir: &Path, key: &LayoutKey) -> Option<Layout> {
    let bytes = std::fs::read(dir.join(file_name(key))).ok()?;
    let layout: Layout = serde_json::from_slice(&bytes).ok()?;
    (layout.version == VERSION).then_some(layout)
}

fn save_in(dir: &Path, key: &LayoutKey, layout: &Layout) {
    if let Ok(bytes) = serde_json::to_vec(layout) {
        // A layout that fails to save is only forgotten, never an error.
        let _ = write_atomic(&dir.join(file_name(key)), &bytes);
    }
}

pub(crate) fn capture(
    graph: &Graph,
    anchors: &HashMap<NodeId, String>,
    view: Option<View>,
) -> Layout {
    let nodes = graph
        .nodes
        .iter()
        .filter_map(|n| Some((anchors.get(&n.id)?.clone(), (n.x, n.y))))
        .collect();
    let groups = graph
        .groups
        .iter()
        .map(|g| SavedGroup {
            title: g.title.clone(),
            x: g.x,
            y: g.y,
            w: g.w,
            h: g.h,
            members: graph
                .nodes
                .iter()
                .filter(|n| n.x >= g.x && n.x <= g.x + g.w && n.y >= g.y && n.y <= g.y + g.h)
                .filter_map(|n| anchors.get(&n.id).cloned())
                .collect(),
        })
        .collect();
    Layout {
        version: VERSION,
        nodes,
        groups,
        view,
    }
}

/// Moves every node whose anchor was saved, rebuilds the saved groups, and
/// returns the nodes it placed; the caller places the rest.
pub(crate) fn apply(
    graph: &mut Graph,
    anchors: &HashMap<NodeId, String>,
    layout: &Layout,
) -> BTreeSet<NodeId> {
    let mut placed = BTreeSet::new();
    let mut live: BTreeSet<&str> = BTreeSet::new();
    for node in &mut graph.nodes {
        let Some(anchor) = anchors.get(&node.id) else {
            continue;
        };
        live.insert(anchor);
        if let Some(&(x, y)) = layout.nodes.get(anchor) {
            node.x = x;
            node.y = y;
            placed.insert(node.id);
        }
    }
    graph.groups = layout
        .groups
        .iter()
        .filter(|g| g.members.is_empty() || g.members.iter().any(|m| live.contains(m.as_str())))
        .map(|g| Group {
            title: g.title.clone(),
            x: g.x,
            y: g.y,
            w: g.w,
            h: g.h,
        })
        .collect();
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_editor::graph::Node;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rbx-saved-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn key() -> LayoutKey {
        LayoutKey {
            place: "cloud:1".into(),
            script: "ServerScriptService.Main".into(),
        }
    }

    fn node(id: NodeId, x: f32, y: f32) -> Node {
        Node {
            id,
            kind: "x".into(),
            x,
            y,
            values: BTreeMap::new(),
        }
    }

    fn anchors(pairs: &[(NodeId, &str)]) -> HashMap<NodeId, String> {
        pairs.iter().map(|&(i, a)| (i, a.to_owned())).collect()
    }

    #[test]
    fn round_trip_and_bad_files() {
        let d = dir("rt");
        assert!(load_from(&d, &key()).is_none());
        let layout = Layout {
            version: VERSION,
            nodes: [("a".to_owned(), (1.0, 2.0))].into(),
            groups: vec![],
            view: Some(View {
                zoom: 2.0,
                pan_x: 3.0,
                pan_y: 4.0,
            }),
        };
        save_in(&d, &key(), &layout);
        assert_eq!(load_from(&d, &key()), Some(layout));
        let file = d.join(file_name(&key()));
        std::fs::write(&file, "{ nope").unwrap();
        assert!(load_from(&d, &key()).is_none());
        std::fs::write(&file, r#"{"version":2,"nodes":{},"groups":[],"view":null}"#).unwrap();
        assert!(load_from(&d, &key()).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn anchors_survive_an_edit() {
        let mut g = Graph {
            nodes: vec![
                node(1, 10.0, 10.0),
                node(2, 200.0, 10.0),
                node(3, 400.0, 10.0),
            ],
            ..Graph::default()
        };
        g.groups.push(Group {
            title: "G".into(),
            x: 0.0,
            y: 0.0,
            w: 250.0,
            h: 50.0,
        });
        let layout = capture(&g, &anchors(&[(1, "a"), (2, "b"), (3, "c")]), None);
        assert_eq!(layout.groups[0].members, ["a", "b"]);

        // Edit: "b" removed, "n" added, every id renumbered, positions reset.
        let mut h = Graph {
            nodes: vec![node(7, 0.0, 0.0), node(8, 0.0, 0.0), node(9, 0.0, 0.0)],
            ..Graph::default()
        };
        let placed = apply(&mut h, &anchors(&[(7, "a"), (8, "n"), (9, "c")]), &layout);
        assert_eq!(placed, BTreeSet::from([7, 9]));
        assert_eq!((h.nodes[0].x, h.nodes[2].x), (10.0, 400.0));
        assert_eq!(h.nodes[1].x, 0.0);
        assert_eq!(h.groups.len(), 1);

        // Every member gone: the group goes too.
        let mut k = Graph {
            nodes: vec![node(1, 0.0, 0.0)],
            ..Graph::default()
        };
        apply(&mut k, &anchors(&[(1, "c")]), &layout);
        assert!(k.groups.is_empty());
    }
}
