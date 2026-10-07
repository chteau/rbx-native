//! The canvas's commands that are not drags: the wheel's pan and zoom,
//! fit, duplicate and group.

use gpui_kit::*;
use rbx_dom::Ref;

use super::super::canvas;
use crate::script_editor::graph::catalog;
use crate::script_editor::graph::layout;
use crate::script_editor::graph::{End, Group};

use super::super::super::Shell;

impl Shell {
    pub(super) fn graph_wheel(
        &mut self,
        reference: Ref,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        if editor.menu.is_some() {
            return;
        }
        let delta = event.delta.pixel_delta(px(16.));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.control || event.modifiers.platform {
            let at = editor.panel(event.position);
            editor.view = clamp_zoom(editor.view.zoomed((dy * 0.0025).exp(), at), at);
        } else {
            let (dx, dy) = match event.modifiers.shift && dx == 0.0 {
                true => (dy, 0.0),
                false => (dx, dy),
            };
            editor.view.pan = [editor.view.pan[0] + dx, editor.view.pan[1] + dy];
        }
        editor.fitted = false;
        cx.notify();
    }

    /// Ctrl+D: copies of the selection, with the wires among them, a
    /// little down and to the right.
    pub(super) fn duplicate_graph_selection(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        let graph = &mut editor.graph;
        let mut copies = Vec::new();
        for id in editor.selection.iter() {
            let Some(node) = graph.node(*id).cloned() else {
                continue;
            };
            let Some(kind) = catalog::kind(&node.kind) else {
                continue;
            };
            let copy = graph.add(kind, [node.x + 24.0, node.y + 24.0]);
            if let Some(added) = graph.node_mut(copy) {
                added.values = node.values.clone();
            }
            copies.push((*id, copy));
        }
        let renamed = |id: u32| {
            copies
                .iter()
                .find(|(old, _)| *old == id)
                .map(|(_, new)| *new)
        };
        let inner: Vec<_> = graph
            .wires
            .iter()
            .filter_map(|wire| {
                Some((
                    End::new(renamed(wire.from.node)?, &wire.from.pin),
                    End::new(renamed(wire.to.node)?, &wire.to.pin),
                ))
            })
            .collect();
        for (from, to) in inner {
            let _ = graph.connect(from, to);
        }
        editor.selection = copies.iter().map(|(_, new)| *new).collect();
        self.commit_graph(reference, cx);
    }

    /// Ctrl+G: a frame round the selection, to be retitled from its title.
    pub(super) fn group_graph_selection(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        let Some(area) = editor
            .selection
            .iter()
            .filter_map(|id| editor.graph.node(*id))
            .map(|node| layout::rect(&editor.graph, node))
            .reduce(|a, b| a.union(&b))
        else {
            return;
        };
        const PAD: f32 = 24.0;
        editor.graph.groups.push(Group {
            title: "Group".into(),
            x: area.x - PAD,
            y: area.y - PAD,
            w: area.w + PAD * 2.0,
            h: area.h + PAD * 2.0,
        });
        self.commit_graph(reference, cx);
    }

    pub(in crate::shell) fn graph_zoom_by(
        &mut self,
        reference: Ref,
        factor: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        let panel = editor.panel_size();
        let centre = [panel[0] * 0.5, panel[1] * 0.5];
        editor.view = clamp_zoom(editor.view.zoomed(factor, centre), centre);
        editor.fitted = false;
        cx.notify();
    }

    pub(in crate::shell) fn graph_fit(&mut self, reference: Ref, cx: &mut Context<Self>) {
        if let Some(editor) = self.graphs.get_mut(&reference) {
            canvas::fit_now(editor);
            cx.notify();
        }
    }
}

/// The graph's zoom stays between 25% and 200%: below, text is unreadable;
/// above, nothing more is learned.
fn clamp_zoom(view: crate::ui_canvas::View, at: [f32; 2]) -> crate::ui_canvas::View {
    let zoom = view.zoom.clamp(0.25, 2.0);
    if zoom == view.zoom {
        return view;
    }
    view.zoomed(zoom / view.zoom, at)
}
