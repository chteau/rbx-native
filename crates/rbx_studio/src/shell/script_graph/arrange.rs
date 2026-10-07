//! Where nodes sit, as opposed to what the code says: remembering it between
//! sessions, tidying it ("Optimize graph") and taking a move or a tidy back.
//! None of it touches `Source`, so none of it is a DOM undo step; a small
//! stack of layouts in the editor answers Ctrl+Z first, while the last thing
//! done was a layout change.

use gpui_kit::*;
use rbx_dom::Ref;

use crate::script_editor::graph::codegen;
use crate::script_editor::graph::layout as graph_layout;
use crate::script_editor::graph::saved;
use crate::script_editor::graph::{Group, NodeId};
use crate::script_editor::tabs::View as TabView;

use super::super::Shell;
use super::{canvas, GraphEditor};

/// How long after the last zoom or pan step the view is written down.
const SAVE_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

/// Every node's position and every frame, as they were.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LayoutSnapshot {
    nodes: Vec<(NodeId, f32, f32)>,
    groups: Vec<Group>,
}

impl GraphEditor {
    fn snapshot(&self) -> LayoutSnapshot {
        let mut nodes: Vec<_> = self.graph.nodes.iter().map(|n| (n.id, n.x, n.y)).collect();
        nodes.sort_by_key(|(id, ..)| *id);
        LayoutSnapshot {
            nodes,
            groups: self.graph.groups.clone(),
        }
    }

    fn restore(&mut self, snapshot: &LayoutSnapshot) {
        for &(id, x, y) in &snapshot.nodes {
            if let Some(node) = self.graph.node_mut(id) {
                (node.x, node.y) = (x, y);
            }
        }
        self.graph.groups = snapshot.groups.clone();
        self.group = self.group.filter(|&i| i < self.graph.groups.len());
    }

    /// Remembers the layout as it is, to come back to with Ctrl+Z.
    pub(super) fn push_layout(&mut self) {
        let snapshot = self.snapshot();
        self.layout_undo.push(snapshot);
    }

    /// A drag ended: keep the step it pushed if it moved anything.
    pub(super) fn settle_layout(&mut self) {
        match self.layout_undo.last() {
            Some(last) if *last == self.snapshot() => {
                self.layout_undo.pop();
            }
            Some(_) => self.layout_last = true,
            None => {}
        }
    }

    /// Writes the layout and view to this machine's store.
    pub(super) fn save_layout(&self) {
        let Some(key) = &self.key else {
            return;
        };
        let view = saved::View {
            zoom: self.view.zoom,
            pan_x: self.view.pan[0],
            pan_y: self.view.pan[1],
        };
        let anchors = codegen::anchors(&self.graph);
        saved::save(key, &saved::capture(&self.graph, &anchors, Some(view)));
    }
}

impl Shell {
    /// Zoom or pan moved: write the view once it has stopped moving.
    pub(super) fn save_layout_soon(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        editor.save_epoch += 1;
        let epoch = editor.save_epoch;
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = shell.update(cx, |shell, _| {
                if let Some(editor) = shell.graphs.get(&reference) {
                    if editor.save_epoch == epoch {
                        editor.save_layout();
                    }
                }
            });
        })
        .detach();
    }

    /// Optimize graph: the tidy layout, remembered, and one Ctrl+Z step.
    pub(crate) fn optimize_graph(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        editor.push_layout();
        graph_layout::tidy(&mut editor.graph);
        editor.settle_layout();
        editor.context = None;
        canvas::fit_now(editor);
        editor.save_layout();
        cx.notify();
    }

    /// Ctrl+Z in the Graph side: takes back the last layout change when that
    /// was the last thing done. False means it is the DOM's to undo.
    pub(crate) fn undo_graph_layout(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(reference) = self.scripts.tabs.active() else {
            return false;
        };
        if self.scripts.tabs.view(reference) != TabView::Graph {
            return false;
        }
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return false;
        };
        if !editor.layout_last {
            return false;
        }
        let Some(snapshot) = editor.layout_undo.pop() else {
            editor.layout_last = false;
            return false;
        };
        editor.restore(&snapshot);
        editor.layout_last = !editor.layout_undo.is_empty();
        editor.save_layout();
        cx.notify();
        true
    }
}
