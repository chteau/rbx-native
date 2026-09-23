//! What the window shows of a batch: which nodes pass the filter and the
//! search, which rows the list draws in which order, and the numbers the
//! rows and the header quote. Pure, so it is unit-tested on a small tree.

use std::collections::HashSet;

use crate::shell::argon_sync::{ChangeKind, DiffNode};

/// The header's segmented control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Filter {
    All,
    Added,
    Updated,
    Removed,
}

impl Filter {
    pub(super) const ALL: [Filter; 4] =
        [Filter::All, Filter::Added, Filter::Updated, Filter::Removed];

    pub(super) fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Added => "Added",
            Filter::Updated => "Updated",
            Filter::Removed => "Removed",
        }
    }

    fn admits(self, kind: ChangeKind) -> bool {
        match self {
            Filter::All => true,
            Filter::Added => kind == ChangeKind::Added,
            Filter::Updated => kind == ChangeKind::Updated,
            Filter::Removed => kind == ChangeKind::Removed,
        }
    }
}

/// One drawn row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Row {
    Section {
        kind: ChangeKind,
        count: usize,
        open: bool,
    },
    /// A change: `depth` 0 is a root row, deeper ones an addition's
    /// descendants.
    Node {
        id: usize,
        depth: usize,
        has_children: bool,
        open: bool,
    },
}

impl Row {
    pub(super) fn node_id(&self) -> Option<usize> {
        match self {
            Row::Node { id, .. } => Some(*id),
            Row::Section { .. } => None,
        }
    }
}

/// How the list is folded and narrowed.
pub(super) struct View<'a> {
    pub(super) filter: Filter,
    /// Lower-cased, trimmed; empty for no search.
    pub(super) query: &'a str,
    pub(super) collapsed: &'a HashSet<ChangeKind>,
    pub(super) expanded: &'a HashSet<usize>,
}

/// Root changes per kind, before any filter: what the segmented control
/// and the dock's review line count.
pub(super) fn counts(nodes: &[DiffNode]) -> [usize; 4] {
    let mut counts = [0; 4];
    for node in nodes {
        counts[0] += 1;
        counts[match node.kind {
            ChangeKind::Added => 1,
            ChangeKind::Updated => 2,
            ChangeKind::Removed => 3,
        }] += 1;
    }
    counts
}

/// A node matches when its name or path holds the query; with no query,
/// everything matches.
fn matches(node: &DiffNode, query: &str) -> bool {
    query.is_empty()
        || node.name.to_lowercase().contains(query)
        || node.path.to_lowercase().contains(query)
}

/// Whether a node or anything under it matches.
fn subtree_matches(node: &DiffNode, query: &str) -> bool {
    matches(node, query)
        || node
            .children
            .iter()
            .any(|child| subtree_matches(child, query))
}

/// The list's rows, in order.
pub(super) fn rows(nodes: &[DiffNode], view: &View) -> Vec<Row> {
    let mut rows = Vec::new();
    for kind in [ChangeKind::Added, ChangeKind::Updated, ChangeKind::Removed] {
        if !view.filter.admits(kind) {
            continue;
        }
        let roots: Vec<&DiffNode> = nodes
            .iter()
            .filter(|node| node.kind == kind && subtree_matches(node, view.query))
            .collect();
        if roots.is_empty() {
            continue;
        }
        let open = !view.collapsed.contains(&kind);
        rows.push(Row::Section {
            kind,
            count: roots.len(),
            open,
        });
        if !open {
            continue;
        }
        for root in roots {
            push_node(root, 0, view, &mut rows);
        }
    }
    rows
}

fn push_node(node: &DiffNode, depth: usize, view: &View, rows: &mut Vec<Row>) {
    let shown: Vec<&DiffNode> = node
        .children
        .iter()
        .filter(|child| subtree_matches(child, view.query))
        .collect();
    let has_children = !node.children.is_empty();
    // A search that reaches into a subtree opens it; otherwise the user's
    // own expansion state decides.
    let open = has_children
        && (view.expanded.contains(&node.id)
            || (!view.query.is_empty()
                && shown.iter().any(|child| subtree_matches(child, view.query))
                && !matches(node, view.query)));
    rows.push(Row::Node {
        id: node.id,
        depth,
        has_children,
        open,
    });
    if open {
        for child in shown {
            push_node(child, depth + 1, view, rows);
        }
    }
}

/// Every node in the tree, with its parent's id.
pub(super) fn walk(nodes: &[DiffNode]) -> Vec<(&DiffNode, Option<usize>)> {
    fn visit<'a>(
        node: &'a DiffNode,
        parent: Option<usize>,
        out: &mut Vec<(&'a DiffNode, Option<usize>)>,
    ) {
        out.push((node, parent));
        for child in &node.children {
            visit(child, Some(node.id), out);
        }
    }
    let mut out = Vec::new();
    for node in nodes {
        visit(node, None, &mut out);
    }
    out
}

pub(super) fn find(nodes: &[DiffNode], id: usize) -> Option<&DiffNode> {
    walk(nodes)
        .into_iter()
        .find(|(node, _)| node.id == id)
        .map(|(node, _)| node)
}

/// A node by name, for the capture variable.
pub(super) fn find_by_name(nodes: &[DiffNode], name: &str) -> Option<usize> {
    walk(nodes)
        .into_iter()
        .find(|(node, _)| node.name == name)
        .map(|(node, _)| node.id)
}

/// The ids on the way from a root down to `id`, the root first.
pub(super) fn ancestors(nodes: &[DiffNode], id: usize) -> Vec<usize> {
    let all = walk(nodes);
    let mut chain = Vec::new();
    let mut current = all
        .iter()
        .find(|(node, _)| node.id == id)
        .and_then(|(_, parent)| *parent);
    while let Some(parent) = current {
        chain.push(parent);
        current = all
            .iter()
            .find(|(node, _)| node.id == parent)
            .and_then(|(_, grand)| *grand);
    }
    chain.reverse();
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: usize, kind: ChangeKind, name: &str, children: Vec<DiffNode>) -> DiffNode {
        DiffNode {
            id,
            kind,
            name: name.to_owned(),
            class: "Folder".to_owned(),
            path: "Workspace".to_owned(),
            properties: Vec::new(),
            source: None,
            nested: children.len(),
            contents: Vec::new(),
            children,
        }
    }

    fn tree() -> Vec<DiffNode> {
        vec![
            node(
                0,
                ChangeKind::Added,
                "Lobby",
                vec![
                    node(1, ChangeKind::Added, "Floor", vec![]),
                    node(
                        2,
                        ChangeKind::Added,
                        "Portal",
                        vec![node(3, ChangeKind::Added, "Glow", vec![])],
                    ),
                ],
            ),
            node(4, ChangeKind::Updated, "SpawnLocation", vec![]),
            node(5, ChangeKind::Removed, "OldShop", vec![]),
        ]
    }

    fn view<'a>(
        filter: Filter,
        query: &'a str,
        collapsed: &'a HashSet<ChangeKind>,
        expanded: &'a HashSet<usize>,
    ) -> View<'a> {
        View {
            filter,
            query,
            collapsed,
            expanded,
        }
    }

    #[test]
    fn sections_come_in_order_with_their_root_counts_and_nested_rows_stay_folded() {
        let (collapsed, expanded) = (HashSet::new(), HashSet::new());
        let shown = rows(&tree(), &view(Filter::All, "", &collapsed, &expanded));
        assert_eq!(shown.len(), 6);
        assert!(matches!(
            shown[0],
            Row::Section {
                kind: ChangeKind::Added,
                count: 1,
                open: true
            }
        ));
        assert!(matches!(
            shown[1],
            Row::Node {
                id: 0,
                depth: 0,
                has_children: true,
                open: false
            }
        ));
        assert!(matches!(
            shown[2],
            Row::Section {
                kind: ChangeKind::Updated,
                ..
            }
        ));
        assert_eq!(counts(&tree()), [3, 1, 1, 1]);
    }

    #[test]
    fn an_expanded_root_lists_its_children_at_depth_one() {
        let collapsed = HashSet::new();
        let expanded: HashSet<usize> = [0].into_iter().collect();
        let shown = rows(&tree(), &view(Filter::Added, "", &collapsed, &expanded));
        let ids: Vec<usize> = shown.iter().filter_map(Row::node_id).collect();
        assert_eq!(ids, vec![0, 1, 2]);
        assert!(matches!(
            shown[3],
            Row::Node {
                id: 2,
                depth: 1,
                has_children: true,
                open: false
            }
        ));
    }

    #[test]
    fn a_collapsed_section_keeps_only_its_header() {
        let collapsed: HashSet<ChangeKind> = [ChangeKind::Added].into_iter().collect();
        let expanded = HashSet::new();
        let shown = rows(&tree(), &view(Filter::All, "", &collapsed, &expanded));
        assert!(matches!(shown[0], Row::Section { open: false, .. }));
        assert!(matches!(
            shown[1],
            Row::Section {
                kind: ChangeKind::Updated,
                ..
            }
        ));
    }

    #[test]
    fn a_search_keeps_a_nested_matchs_ancestors_and_opens_them() {
        let (collapsed, expanded) = (HashSet::new(), HashSet::new());
        let shown = rows(&tree(), &view(Filter::All, "glow", &collapsed, &expanded));
        let ids: Vec<usize> = shown.iter().filter_map(Row::node_id).collect();
        assert_eq!(ids, vec![0, 2, 3]);
        assert!(shown.iter().all(|row| !matches!(
            row,
            Row::Section {
                kind: ChangeKind::Updated,
                ..
            }
        )));
        assert!(rows(
            &tree(),
            &view(Filter::All, "nothing", &collapsed, &expanded)
        )
        .is_empty());
    }

    #[test]
    fn ancestors_run_root_first() {
        assert_eq!(ancestors(&tree(), 3), vec![0, 2]);
        assert!(ancestors(&tree(), 4).is_empty());
        assert_eq!(find_by_name(&tree(), "Glow"), Some(3));
    }
}
