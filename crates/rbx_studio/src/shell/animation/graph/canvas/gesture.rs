//! The canvas's pointer gestures: moving a node, panning, and dragging a
//! wire from an output to an input.

use gpui_kit::*;

use super::super::GraphGesture;
use super::Shell;
use crate::anim_editor::graph_canvas::{self, Hit};

impl Shell {
    pub(in crate::shell::animation) fn anim_graph_press(
        &mut self,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = self.anim_graph_point(at) else {
            return;
        };
        let doc = &mut self.anim.graph;
        doc.gesture = match graph_canvas::hit(&doc.graph, p) {
            Hit::Output(n) => Some(GraphGesture::Wire { from: n, to: p }),
            Hit::Input(n, k) => {
                // Pulling a wired input picks its wire up from the source end.
                let source = doc.graph.nodes[n].inputs[k].node;
                let pin = doc.graph.nodes[n].inputs[k].pin.clone();
                source.map(|from| {
                    doc.edit(|g| g.disconnect(n, &pin));
                    GraphGesture::Wire { from, to: p }
                })
            }
            Hit::Node(n) => {
                doc.selected = Some(n);
                let (x, y) = doc.graph.nodes[n].position;
                Some(GraphGesture::MoveNode {
                    node: n,
                    grab: [p[0] - x, p[1] - y],
                })
            }
            Hit::Empty => {
                doc.selected = None;
                Some(GraphGesture::Pan {
                    last: [f32::from(at.x), f32::from(at.y)],
                })
            }
        };
        cx.notify();
    }

    pub(in crate::shell::animation) fn anim_graph_drag(
        &mut self,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let p = self.anim_graph_point(at);
        let doc = &mut self.anim.graph;
        match (doc.gesture, p) {
            (Some(GraphGesture::MoveNode { node, grab }), Some(p)) => {
                if let Some(n) = doc.graph.nodes.get_mut(node) {
                    n.position = (p[0] - grab[0], p[1] - grab[1]);
                    doc.dirty = true;
                }
            }
            (Some(GraphGesture::Pan { last }), _) => {
                let now = [f32::from(at.x), f32::from(at.y)];
                for a in 0..2 {
                    doc.view.pan[a] += now[a] - last[a];
                }
                doc.gesture = Some(GraphGesture::Pan { last: now });
            }
            (Some(GraphGesture::Wire { from, .. }), Some(p)) => {
                doc.gesture = Some(GraphGesture::Wire { from, to: p });
            }
            _ => return,
        }
        cx.notify();
    }

    pub(in crate::shell::animation) fn anim_graph_release(
        &mut self,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let p = self.anim_graph_point(at);
        let doc = &mut self.anim.graph;
        let Some(gesture) = doc.gesture.take() else {
            return;
        };
        if let (GraphGesture::Wire { from, .. }, Some(p)) = (gesture, p) {
            if let Hit::Input(n, k) = graph_canvas::hit(&doc.graph, p) {
                let pin = doc.graph.nodes[n].inputs[k].pin.clone();
                let mut ok = false;
                doc.edit(|g| ok = g.connect(from, n, &pin));
                if !ok {
                    self.anim.warn("That wire would make a loop");
                }
            }
        }
        cx.notify();
    }
}
