//! What the window keeps between frames: the tree for the pending
//! review, each script's diff, the card's rows, the selection and the
//! keyboard.

use std::rc::Rc;

use gpui_kit::*;

use crate::shell::argon_sync::{ChangeKind, DiffNode};

use super::code::{self, CodeRow, Source};
use super::model::{self, Row};
use super::{diff, ArgonDiffWindow, CachedDiff};

impl ArgonDiffWindow {
    /// The tree for the pending review, rebuilt when its serial changes.
    pub(super) fn nodes(&mut self, cx: &App) -> Rc<Vec<DiffNode>> {
        let shell = self.shell.read(cx);
        let serial = shell.argon_pending_serial().unwrap_or(0);
        match &self.nodes {
            Some((cached, nodes)) if *cached == serial => nodes.clone(),
            _ => {
                let nodes = Rc::new(shell.argon_diff_nodes());
                self.nodes = Some((serial, nodes.clone()));
                self.diffs.retain(|(s, _), _| *s == serial);
                nodes
            }
        }
    }

    pub(super) fn serial(&self, cx: &App) -> u64 {
        self.shell.read(cx).argon_pending_serial().unwrap_or(0)
    }

    /// The search as typed, trimmed.
    pub(super) fn query_text(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_owned()
    }

    pub(super) fn query_lower(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
    }

    pub(super) fn rows(&self, nodes: &[DiffNode], cx: &App) -> Vec<Row> {
        let query = self.query_lower(cx);
        model::rows(
            nodes,
            &model::View {
                filter: self.filter,
                query: &query,
                collapsed: &self.collapsed,
                expanded: &self.expanded,
            },
        )
    }

    /// A script's diff for `node`, from the cache or computed now.
    pub(super) fn cached_diff(&mut self, node: &DiffNode, cx: &App) -> Option<Rc<CachedDiff>> {
        let source = node.source.as_ref()?;
        let key = (self.serial(cx), node.id);
        if let Some(cached) = self.diffs.get(&key) {
            return Some(cached.clone());
        }
        let old = Source::new(source.old.as_deref().unwrap_or_default());
        let new = Source::new(source.new.as_deref().unwrap_or_default());
        let old_lines: Vec<String> = source
            .old
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        let new_lines: Vec<String> = source
            .new
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        let diff = diff::diff_lines(
            &old_lines.iter().map(String::as_str).collect::<Vec<_>>(),
            &new_lines.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        let cached = Rc::new(CachedDiff { old, new, diff });
        self.diffs.insert(key, cached.clone());
        Some(cached)
    }

    /// `(added, removed)` lines for a script node's row and header.
    pub(super) fn line_counts(&mut self, node: &DiffNode) -> (usize, usize) {
        let Some(source) = &node.source else {
            return (0, 0);
        };
        match (&source.old, &source.new) {
            (Some(_), Some(_)) => {
                let key = self.nodes.as_ref().map(|(serial, _)| (*serial, node.id));
                key.and_then(|key| self.diffs.get(&key))
                    .map(|cached| (cached.diff.added, cached.diff.removed))
                    .unwrap_or((0, 0))
            }
            (None, Some(new)) => (new.lines().count(), 0),
            (Some(old), None) => (0, old.lines().count()),
            (None, None) => (0, 0),
        }
    }

    /// The card's rows for `node`, rebuilt when the batch, the node or the
    /// opened hunks change; the list is reset with them.
    pub(super) fn rows_for(&mut self, node: &DiffNode, cx: &mut Context<Self>) -> Rc<Vec<CodeRow>> {
        let key = (self.serial(cx), node.id);
        if let Some((cached, rows)) = &self.code_rows {
            if *cached == key {
                return rows.clone();
            }
        }
        let limit = self.shell.read(cx).argon_diff_lines_limit();
        let rows = match self.cached_diff(node, cx) {
            Some(cached)
                if node
                    .source
                    .as_ref()
                    .is_some_and(|s| s.old.is_some() && s.new.is_some()) =>
            {
                code::unified_rows(&cached.old, &cached.new, &cached.diff, &self.expanded_hunks)
            }
            Some(cached) if node.source.as_ref().is_some_and(|s| s.new.is_some()) => {
                code::plain_rows(&cached.new)
            }
            Some(cached) => code::plain_rows(&cached.old),
            None => Vec::new(),
        };
        let rows = Rc::new(code::cap(rows, limit));
        self.code_list.reset(rows.len());
        self.code_rows = Some((key, rows.clone()));
        rows
    }

    /// A hunk row's click: its hidden lines come in as context rows.
    pub(super) fn expand_hunk(&mut self, key: usize) {
        self.expanded_hunks.insert(key);
        self.code_rows = None;
    }

    pub(super) fn select(&mut self, id: usize, _cx: &mut Context<Self>) {
        if self.selected != Some(id) {
            self.selected = Some(id);
            self.expanded_hunks.clear();
            self.code_rows = None;
        }
        self.picker_open = false;
    }

    pub(super) fn toggle_expanded(&mut self, id: usize) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
    }

    /// Keeps the selection on a visible row: the first one when the
    /// current selection is filtered, searched or folded away.
    pub(super) fn ensure_selection(&mut self, cx: &mut Context<Self>) {
        let nodes = self.nodes(cx);
        let rows = self.rows(&nodes, cx);
        let visible: Vec<usize> = rows.iter().filter_map(Row::node_id).collect();
        if self.selected.is_some_and(|id| visible.contains(&id)) {
            return;
        }
        match visible.first() {
            Some(&first) => self.select(first, cx),
            None => self.selected = None,
        }
    }

    /// The capture variables, and the default selection, on the first
    /// frame the tree is known.
    pub(super) fn apply_initial(&mut self, nodes: &[DiffNode], cx: &mut Context<Self>) {
        let Some((select, collapse, expand)) = self.initial.take() else {
            return;
        };
        for section in collapse {
            let kind = match section.to_lowercase().as_str() {
                "additions" => ChangeKind::Added,
                "updates" => ChangeKind::Updated,
                "removals" => ChangeKind::Removed,
                _ => continue,
            };
            self.collapsed.insert(kind);
        }
        for name in expand {
            if let Some(id) = model::find_by_name(nodes, &name) {
                self.expanded.insert(id);
            }
        }
        if let Some(id) = select.and_then(|name| model::find_by_name(nodes, &name)) {
            for ancestor in model::ancestors(nodes, id) {
                self.expanded.insert(ancestor);
            }
            self.select(id, cx);
        }
        self.ensure_selection(cx);
    }

    /// ↑/↓ walk the visible rows, ←/→ fold and unfold, Esc closes.
    pub(super) fn on_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let nodes = self.nodes(cx);
        let rows = self.rows(&nodes, cx);
        let visible: Vec<usize> = rows.iter().filter_map(Row::node_id).collect();
        let position = self
            .selected
            .and_then(|id| visible.iter().position(|&row| row == id));
        match keystroke.key.as_str() {
            "escape" => {
                window.remove_window();
                true
            }
            "down" => {
                let next =
                    position.map_or(0, |index| (index + 1).min(visible.len().saturating_sub(1)));
                if let Some(&id) = visible.get(next) {
                    self.select(id, cx);
                }
                true
            }
            "up" => {
                let next = position.map_or(0, |index| index.saturating_sub(1));
                if let Some(&id) = visible.get(next) {
                    self.select(id, cx);
                }
                true
            }
            "right" | "left" => {
                if let Some(id) = self.selected {
                    let open = keystroke.key == "right";
                    let has_children =
                        model::find(&nodes, id).is_some_and(|node| !node.children.is_empty());
                    if has_children {
                        if open {
                            self.expanded.insert(id);
                        } else {
                            self.expanded.remove(&id);
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }
}
