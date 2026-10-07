//! The Figma window's pure half: the file tree as rows, and the review
//! step's edits laid over the inferred tree.

use std::collections::{HashMap, HashSet};

use rbx_figma::browse::Item;
use rbx_figma::infer::{Confidence, Node};

/// One visible row of the file tree.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TreeRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub depth: usize,
    pub expandable: bool,
    pub expanded: bool,
}

/// The rows to draw: expanded branches open, or, with a `filter`, every
/// loaded row whose name holds it (case-blind) at its own depth.
pub(super) fn tree_rows(pages: &[Item], expanded: &HashSet<String>, filter: &str) -> Vec<TreeRow> {
    fn walk(
        items: &[Item],
        depth: usize,
        expanded: &HashSet<String>,
        filter: &str,
        out: &mut Vec<TreeRow>,
    ) {
        for item in items {
            let open = expanded.contains(&item.id);
            if filter.is_empty() || item.name.to_lowercase().contains(filter) {
                out.push(TreeRow {
                    id: item.id.clone(),
                    name: item.name.clone(),
                    kind: item.kind.clone(),
                    depth,
                    expandable: item.expandable(),
                    expanded: open,
                });
            }
            if open || !filter.is_empty() {
                let children = item.children.as_deref().unwrap_or_default();
                walk(children, depth + 1, expanded, filter, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(pages, 0, expanded, &filter.trim().to_lowercase(), &mut out);
    out
}

/// Figma's `COMPONENT_SET` as "Component set"; a `CANVAS` is a page.
pub(super) fn kind_label(kind: &str) -> String {
    if kind == "CANVAS" {
        return "Page".into();
    }
    let lower = kind.replace('_', " ").to_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// What review changed, by Figma node id. Kept apart from the inferred
/// tree so un-flattening gives the children back.
#[derive(Debug, Default)]
pub(super) struct Edits {
    classes: HashMap<String, &'static str>,
    flat: HashSet<String>,
}

impl Edits {
    /// Picks `class` for `id`; picking the inferred one again drops the edit.
    pub fn set_class(&mut self, id: &str, class: &'static str, inferred: &'static str) {
        if class == inferred {
            self.classes.remove(id);
        } else {
            self.classes.insert(id.to_string(), class);
        }
    }

    pub fn toggle_flat(&mut self, id: &str) {
        if !self.flat.remove(id) {
            self.flat.insert(id.to_string());
        }
    }

    pub fn is_flat(&self, id: &str) -> bool {
        self.flat.contains(id)
    }
}

/// `tree` (the inferred one) with every edit applied: what Import uploads
/// and inserts, and what the review list draws.
pub(super) fn apply(mut tree: Node, edits: &Edits) -> Node {
    tree.walk_mut(&mut |node| {
        if edits.flat.contains(&node.id) {
            node.flatten();
        } else if let Some(class) = edits.classes.get(&node.id) {
            node.set_class(class);
        }
    });
    tree
}

/// The node with Figma id `id` anywhere under (or at) `node`.
pub(super) fn find<'a>(node: &'a Node, id: &str) -> Option<&'a Node> {
    if node.id == id {
        return Some(node);
    }
    node.children.iter().find_map(|c| find(c, id))
}

/// One design node of the review list.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ReviewRow {
    pub id: String,
    pub name: String,
    pub depth: usize,
    pub class: &'static str,
    pub confidence: Confidence,
    pub notes: Option<String>,
    pub flattened: bool,
    /// The `UI*` modifiers inference added under it, by class.
    pub modifiers: Vec<&'static str>,
}

/// The design nodes of `applied` (see [`apply`]), parents first; a
/// flattened node's subtree is gone, as it will be in the place.
pub(super) fn review_rows(applied: &Node, edits: &Edits) -> Vec<ReviewRow> {
    fn walk(node: &Node, depth: usize, edits: &Edits, out: &mut Vec<ReviewRow>) {
        out.push(ReviewRow {
            id: node.id.clone(),
            name: node.name.clone(),
            depth,
            class: node.class,
            confidence: node.confidence,
            notes: node.review.clone(),
            flattened: edits.is_flat(&node.id),
            modifiers: node
                .children
                .iter()
                .filter(|c| !c.is_design())
                .map(|c| c.class)
                .collect(),
        });
        for child in node.children.iter().filter(|c| c.is_design()) {
            walk(child, depth + 1, edits, out);
        }
    }
    let mut out = Vec::new();
    if applied.is_design() {
        walk(applied, 0, edits, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{apply, kind_label, review_rows, tree_rows, Edits};
    use rbx_figma::browse::Item;
    use rbx_figma::infer::{infer, Confidence};
    use std::collections::HashSet;

    fn item(id: &str, name: &str, kind: &str, children: Option<Vec<Item>>) -> Item {
        Item {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            children,
        }
    }

    fn pages() -> Vec<Item> {
        vec![item(
            "0:1",
            "Page 1",
            "CANVAS",
            Some(vec![item(
                "1:1",
                "Shop",
                "FRAME",
                Some(vec![item("1:2", "Buy button", "INSTANCE", None)]),
            )]),
        )]
    }

    #[test]
    fn only_expanded_branches_show_their_rows() {
        let ids = |rows: Vec<super::TreeRow>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
        let mut open = HashSet::new();
        assert_eq!(ids(tree_rows(&pages(), &open, "")), ["0:1"]);
        open.insert("0:1".to_string());
        open.insert("1:1".to_string());
        let rows = tree_rows(&pages(), &open, "");
        assert_eq!(rows[2].depth, 2);
        assert!(rows[2].expandable, "an unloaded instance can still open");
        assert_eq!(ids(rows), ["0:1", "1:1", "1:2"]);
    }

    #[test]
    fn a_filter_searches_every_loaded_row() {
        let rows = tree_rows(&pages(), &HashSet::new(), " BUY ");
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].id.as_str(), rows[0].depth), ("1:2", 2));
    }

    #[test]
    fn kinds_read_as_words() {
        assert_eq!(kind_label("CANVAS"), "Page");
        assert_eq!(kind_label("COMPONENT_SET"), "Component set");
        assert_eq!(kind_label("TEXT"), "Text");
    }

    fn tree() -> rbx_figma::infer::Node {
        let text = serde_json::json!({
            "id": "2:1", "type": "TEXT", "name": "Title", "characters": "Shop",
            "absoluteBoundingBox": { "x": 10, "y": 10, "width": 80, "height": 20 },
            "style": { "fontFamily": "Inter", "fontSize": 18 },
        });
        infer(&serde_json::json!({
            "id": "1:1", "type": "FRAME", "name": "Shop", "cornerRadius": 8,
            "absoluteBoundingBox": { "x": 0, "y": 0, "width": 400, "height": 300 },
            "children": [text],
        }))
        .unwrap()
    }

    #[test]
    fn review_lists_design_nodes_and_sums_up_modifiers() {
        let tree = tree();
        let edits = Edits::default();
        let rows = review_rows(&apply(tree, &edits), &edits);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].class, rows[0].depth), ("Frame", 0));
        assert!(rows[0].modifiers.contains(&"UICorner"));
        assert_eq!((rows[1].class, rows[1].depth), ("TextLabel", 1));
    }

    #[test]
    fn edits_apply_and_undo_cleanly() {
        let tree = tree();
        let mut edits = Edits::default();
        edits.set_class("2:1", "TextButton", "TextLabel");
        let rows = review_rows(&apply(tree.clone(), &edits), &edits);
        assert_eq!(rows[1].class, "TextButton");
        assert_eq!(rows[1].confidence, Confidence::High);

        edits.toggle_flat("1:1");
        let flat = apply(tree.clone(), &edits);
        assert!(flat.children.is_empty());
        let rows = review_rows(&flat, &edits);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].flattened && rows[0].class == "ImageLabel");

        edits.toggle_flat("1:1");
        edits.set_class("2:1", "TextLabel", "TextLabel");
        assert_eq!(apply(tree.clone(), &edits), tree);
    }
}
