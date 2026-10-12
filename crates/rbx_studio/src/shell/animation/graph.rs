//! The Animation Graph sub-tab: an `AnimationGraphDefinition` edited as
//! nodes and wires on a canvas, played on the bound rig with live
//! parameters. Like the clip, the graph is a working copy until saved.

mod canvas;
mod panel;

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use rbx_animation::graph::{self, Graph, NodeKind, Player, Prop, Value};
use rbx_animation::{sequence, Clip, Frame};
use rbx_dom::Ref;

use super::Shell;
use crate::anim_editor::{graph_canvas, place};
use crate::ui_canvas::View;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum GraphGesture {
    MoveNode { node: usize, grab: [f32; 2] },
    Pan { last: [f32; 2] },
    Wire { from: usize, to: [f32; 2] },
}

pub(in crate::shell) struct GraphDoc {
    pub(super) graph: Graph,
    pub(super) source: Option<Ref>,
    pub(super) view: View,
    pub(super) selected: Option<usize>,
    pub(super) gesture: Option<GraphGesture>,
    pub(super) player: Option<Player>,
    pub(super) playing: bool,
    /// The graph's latest output, which poses the rig while it plays.
    pub(super) pose: Option<BTreeMap<String, Frame>>,
    pub(super) bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// One field per editable value, by a key naming what it edits.
    pub(super) inputs: HashMap<String, Entity<InputState>>,
    pub(super) subscriptions: Vec<Subscription>,
    pub(super) dirty: bool,
    pub(super) fitted: bool,
}

impl Default for GraphDoc {
    fn default() -> Self {
        GraphDoc {
            graph: Graph::default(),
            source: None,
            view: View {
                zoom: 1.0,
                pan: [0.0, 0.0],
            },
            selected: None,
            gesture: None,
            player: None,
            playing: false,
            pose: None,
            bounds: Rc::new(Cell::new(None)),
            inputs: HashMap::new(),
            subscriptions: Vec::new(),
            dirty: false,
            fitted: false,
        }
    }
}

impl GraphDoc {
    /// Applies an edit to the graph, and restarts a running preview on the
    /// new graph so what plays is what is drawn.
    pub(super) fn edit(&mut self, change: impl FnOnce(&mut Graph)) {
        change(&mut self.graph);
        self.dirty = true;
        if self.player.is_some() {
            self.player = None;
        }
    }

    pub(super) fn load(&mut self, mut graph: Graph, source: Option<Ref>) {
        graph_canvas::arrange(&mut graph);
        self.graph = graph;
        self.source = source;
        self.selected = None;
        self.player = None;
        self.playing = false;
        self.pose = None;
        self.dirty = false;
        self.fitted = false;
        self.inputs.clear();
        self.subscriptions.clear();
    }
}

impl Shell {
    /// Every clip the graph's Clip nodes name: a `KeyframeSequence` of that
    /// name in the place (the rig's own saves first), or the clip open in
    /// the dope sheet when it carries the name.
    fn anim_graph_clips(&self) -> HashMap<String, Clip> {
        let rig = self.anim.bound.as_ref().map(|b| b.model);
        let mut out = HashMap::new();
        for node in &self.anim.graph.graph.nodes {
            let Some(Prop::Value(Value::Text(id))) = node.props.get("AnimationId") else {
                continue;
            };
            if out.contains_key(id) || node.kind != NodeKind::Clip {
                continue;
            }
            let clip = if id == &self.anim.editor.clip.name {
                Some(self.anim.editor.clip.clone())
            } else {
                place::sequence_named(&self.dom, id, rig).map(|s| sequence::read(&self.dom, s))
            };
            if let Some(clip) = clip {
                out.insert(id.clone(), clip);
            }
        }
        out
    }

    pub(super) fn anim_toggle_graph_playback(&mut self) {
        let doc = &mut self.anim.graph;
        doc.playing = !doc.playing;
        if !doc.playing {
            return;
        }
        if doc.player.is_none() {
            let clips = self.anim_graph_clips();
            let player = Player::new(self.anim.graph.graph.clone(), &|id| clips.get(id).cloned());
            self.anim.graph.player = Some(player);
        }
    }

    pub(super) fn anim_step_graph(&mut self, dt: f32) {
        if let Some(player) = &mut self.anim.graph.player {
            self.anim.graph.pose = Some(player.step(dt));
        }
    }

    pub(super) fn anim_new_graph(&mut self, cx: &mut Context<Self>) {
        self.anim.graph.load(Graph::default(), None);
        self.anim.say("New animation graph");
        cx.notify();
    }

    pub(super) fn anim_load_graph(&mut self, definition: Ref, cx: &mut Context<Self>) {
        let graph = graph::dom::read(&self.dom, definition);
        self.anim.graph.load(graph, Some(definition));
        cx.notify();
    }

    /// Writes the graph as an `AnimationGraphDefinition` in the rig's save
    /// folder (or over the one it was loaded from), as one undoable edit.
    pub(super) fn anim_save_graph(&mut self, cx: &mut Context<Self>) {
        self.unpose_rig(cx);
        self.push_history();
        let graph = self.anim.graph.graph.clone();
        let existing = self.anim.graph.source.filter(|&s| {
            self.dom
                .get(s)
                .is_some_and(|i| i.class() == "AnimationGraphDefinition")
        });
        let target = match existing {
            Some(s) => {
                graph::dom::overwrite(&mut self.dom, s, &graph);
                s
            }
            None => {
                let folder = self
                    .anim
                    .bound
                    .as_ref()
                    .map(|b| b.model)
                    .map(|m| place::save_folder(&mut self.dom, m));
                graph::dom::write(&mut self.dom, folder, &graph)
            }
        };
        self.anim.graph.source = Some(target);
        self.anim.graph.dirty = false;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.rebuild_explorer(cx);
        self.keep_draft(true);
        self.anim.say(format!(
            "Saved {} as an AnimationGraphDefinition",
            graph.name
        ));
        cx.notify();
    }

    pub(super) fn anim_add_graph_node(&mut self, kind: NodeKind, cx: &mut Context<Self>) {
        let doc = &mut self.anim.graph;
        // Into the middle of what the canvas shows.
        let centre = doc.bounds.get().map_or([0.0, 0.0], |b| {
            let size = [f32::from(b.size.width), f32::from(b.size.height)];
            [0, 1].map(|a| (size[a] * 0.5 - doc.view.pan[a]) / doc.view.zoom)
        });
        let mut index = 0;
        doc.edit(|g| {
            index = g.add(kind);
            g.nodes[index].position =
                (centre[0] - graph_canvas::NODE_WIDTH * 0.5, centre[1] - 30.0);
        });
        doc.selected = Some(index);
        cx.notify();
    }

    pub(super) fn anim_delete_graph_node(&mut self, cx: &mut Context<Self>) {
        let doc = &mut self.anim.graph;
        if let Some(node) = doc.selected.take() {
            let mut removed = false;
            doc.edit(|g| removed = g.remove(node));
            doc.inputs.clear();
            doc.subscriptions.clear();
            if !removed {
                self.anim.warn("The output node stays");
            }
        }
        cx.notify();
    }
}
