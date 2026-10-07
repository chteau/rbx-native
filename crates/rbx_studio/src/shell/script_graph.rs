//! The Script Editor's Graph side: one [`GraphEditor`] per open tab that
//! has shown it, drawing and editing the graph `script_editor::graph`
//! defines.
//!
//! `Source` is the only copy that counts, as it is for code. Opening the
//! side imports the script; every finished edit — a drop, a wire, a typed
//! literal — compiles the graph and, when that differs from `Source`,
//! writes it as one undo step, then imports it again so the graph on screen
//! is what the code says. A change to `Source` from anywhere else (typing in
//! Code, undo, a sync) rebuilds the graph the same way. Positions, groups
//! and the selection are carried over by `graph::sync`, and the layout is
//! remembered per machine by `graph::saved`. A drag in flight edits only
//! the copy here, so a node dragged across the canvas is one step, not one
//! per frame.

mod add_menu;
mod arrange;
mod canvas;
mod context;
mod input;
mod literal;
mod nodes;
mod style;

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use gpui_kit::*;
use rbx_dom::Ref;

use crate::script_editor::graph::catalog::{PinType, Wanted};
use crate::script_editor::graph::codegen::{self, Origins, Problem};
use crate::script_editor::graph::import::Broken;
use crate::script_editor::graph::layout::{self as graph_layout, Handle, Side};
use crate::script_editor::graph::saved::{self, LayoutKey};
use crate::script_editor::graph::{import, sync, End, Graph, Group, NodeId};
use crate::script_editor::source;
use crate::ui_canvas::View;

use super::Shell;

/// What a press on empty canvas does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Tool {
    #[default]
    Select,
    Hand,
    Marquee,
}

/// A drag in flight. Points are canvas units unless named `panel`.
#[derive(Debug, Clone)]
enum Gesture {
    Pan {
        panel: [f32; 2],
    },
    /// The selection, or a group with the nodes inside it, following the
    /// pointer from where it was pressed.
    Move {
        from: [f32; 2],
        origins: Vec<(NodeId, [f32; 2])>,
        group: Option<(usize, [f32; 2])>,
        moved: bool,
    },
    /// A group frame's side or corner dragged; `origin` is the frame as
    /// the press found it.
    Resize {
        index: usize,
        handle: Handle,
        from: [f32; 2],
        origin: Group,
    },
    /// A wire being drawn out of `end`, which sits on `side` of its node.
    Wire {
        end: End,
        side: Side,
        to: [f32; 2],
    },
    Marquee {
        from: [f32; 2],
        to: [f32; 2],
        keep: BTreeSet<NodeId>,
    },
    /// A press in the minimap, centring the view wherever the pointer goes,
    /// on or off the map.
    Minimap(canvas::MinimapFrame),
}

pub(super) struct GraphEditor {
    graph: Graph,
    /// Where each statement of the imported script came from, so one the
    /// graph has not changed compiles back byte for byte.
    origins: Origins,
    /// The `Source` `graph` was last imported from or compiled to. When
    /// `Source` is anything else it changed behind the graph's back, and the
    /// graph is rebuilt.
    built_from: Option<String>,
    /// Set when the script does not parse: it is shown as one code block.
    broken: Option<Broken>,
    /// Where this script's layout is remembered.
    key: Option<LayoutKey>,
    /// Layout changes (a drag, Optimize graph) that Ctrl+Z takes back
    /// before it reaches the DOM's own history.
    layout_undo: Vec<arrange::LayoutSnapshot>,
    /// Whether the last thing done was a layout change.
    layout_last: bool,
    /// Counts layout saves asked for, so only the last of a burst of wheel
    /// events writes.
    save_epoch: u64,
    context: Option<context::ContextMenu>,
    view: View,
    /// Whether the next frame with a size frames the whole graph: set when
    /// the editor is made and by Fit, spent as soon as it is applied.
    fitted: bool,
    selection: BTreeSet<NodeId>,
    /// A group picked by its title: Delete removes the frame, not its nodes.
    group: Option<usize>,
    tool: Tool,
    gesture: Option<Gesture>,
    /// Space is held: a drag pans whatever the tool.
    space: bool,
    /// The last pointer position over the canvas, in panel pixels — where
    /// Shift+A opens the menu.
    pointer: [f32; 2],
    menu: Option<add_menu::AddMenu>,
    literal: Option<literal::LiteralEdit>,
    /// Why the last wire was refused, until the next edit.
    notice: Option<String>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
}

impl GraphEditor {
    fn new(cx: &mut App) -> Self {
        GraphEditor {
            graph: Graph::default(),
            origins: Origins::default(),
            built_from: None,
            broken: None,
            key: None,
            layout_undo: Vec::new(),
            layout_last: false,
            save_epoch: 0,
            context: None,
            view: View {
                zoom: 1.0,
                pan: [24.0, 24.0],
            },
            fitted: true,
            selection: BTreeSet::new(),
            group: None,
            tool: Tool::default(),
            gesture: None,
            space: false,
            pointer: [0.0, 0.0],
            menu: None,
            literal: None,
            notice: None,
            bounds: Rc::default(),
            focus: cx.focus_handle(),
        }
    }

    fn panel_size(&self) -> [f32; 2] {
        let size = self.bounds.get().size;
        [f32::from(size.width), f32::from(size.height)]
    }

    /// A window position as panel pixels.
    fn panel(&self, position: Point<Pixels>) -> [f32; 2] {
        let origin = self.bounds.get().origin;
        [
            f32::from(position.x - origin.x),
            f32::from(position.y - origin.y),
        ]
    }

    /// The graph as `source` says it, laid out as this machine last left
    /// it, or tidied when there is no such layout or `optimize` asks.
    fn open(&mut self, source: &str, optimize: bool) {
        let imported = import::import(source);
        self.graph = imported.graph;
        self.origins = imported.origins;
        self.broken = imported.broken;
        self.built_from = Some(source.to_owned());
        let saved = match optimize {
            true => None,
            false => self.key.as_ref().and_then(saved::load),
        };
        match saved {
            Some(layout) => {
                let anchors = codegen::anchors(&self.graph);
                let placed = saved::apply(&mut self.graph, &anchors, &layout);
                graph_layout::place_new(&mut self.graph, &placed);
                if let Some(view) = layout.view {
                    self.view = View {
                        zoom: view.zoom,
                        pan: [view.pan_x, view.pan_y],
                    };
                    self.fitted = false;
                }
            }
            None => {
                graph_layout::tidy(&mut self.graph);
                self.save_layout();
            }
        }
    }

    /// The graph rebuilt from `source`, which changed outside the graph:
    /// what the edit left alone stays where it was.
    fn rebuild(&mut self, source: &str) {
        let imported = import::import(source);
        let mut graph = imported.graph;
        let carried = sync::carry(&self.graph, &mut graph, &self.selection);
        self.graph = graph;
        self.origins = imported.origins;
        self.broken = imported.broken;
        self.built_from = Some(source.to_owned());
        self.selection = carried.selection;
        self.group = self.group.filter(|&i| i < self.graph.groups.len());
        self.context = None;
        // Snapshots name nodes by number, and the numbers have changed.
        self.layout_undo.clear();
        self.layout_last = false;
        self.save_layout();
    }
}

/// What the status bar reads for the Graph side.
pub(super) struct GraphStatus {
    pub(super) summary: String,
    pub(super) zoom: f32,
}

impl Shell {
    /// The editor for `reference`, made on first use and kept in step with
    /// the script's `Source`.
    fn graph_editor_for(&mut self, reference: Ref, cx: &mut App) -> &mut GraphEditor {
        let text = source::read(&self.dom, reference).unwrap_or_default();
        if !self.graphs.contains_key(&reference) {
            let mut editor = GraphEditor::new(cx);
            editor.key = Some(self.layout_key(reference));
            editor.open(&text, self.optimize_graph_on_open);
            self.graphs.insert(reference, editor);
        }
        let editor = self.graphs.get_mut(&reference).expect("inserted above");
        if editor.gesture.is_none() && editor.built_from.as_deref() != Some(text.as_str()) {
            editor.rebuild(&text);
        }
        editor
    }

    /// Which saved layout is this script's: the place file, or the Roblox
    /// place it was opened from, and the script's path in it.
    fn layout_key(&self, reference: Ref) -> LayoutKey {
        let cloud = crate::home::link_of(&self.path).map(|link| link.place_id);
        saved::key_for(&self.path, cloud, &self.dom, reference)
    }

    /// `RBX_STUDIO_SCRIPT_VIEW=graph=<file>`: the graph in `path` compiled
    /// into `reference`'s `Source`, through the canvas's own commit. A file
    /// that does not hold a graph is ignored.
    pub(super) fn seed_graph(
        &mut self,
        reference: Ref,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let Some(graph) = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| Graph::parse(&text))
        else {
            return;
        };
        let editor = self.graph_editor_for(reference, cx);
        editor.graph = graph;
        editor.origins = Origins::default();
        self.commit_graph(reference, cx);
    }

    /// `RBX_STUDIO_GRAPH_MENU=<query>`: the add menu open with `query`
    /// typed, finishing a wire from the first node's first value output.
    pub(super) fn debug_graph_menu(
        &mut self,
        reference: Ref,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.graph_editor_for(reference, cx);
        // Left at 100% rather than fitted, so the menu opens beside the
        // node the way a real drop would leave it.
        editor.fitted = false;
        let graph = &editor.graph;
        let wire = graph
            .nodes
            .iter()
            .min_by_key(|node| node.id)
            .and_then(|node| {
                let pin = graph
                    .pins(node.id)
                    .outputs
                    .into_iter()
                    .find(|pin| pin.ty != PinType::Exec)?;
                Some((
                    End::new(node.id, pin.name),
                    Side::Output,
                    Wanted::Input(pin.ty),
                ))
            });
        self.open_add_menu(reference, [240.0, 250.0], wire, window, cx);
        let typed = query.to_owned();
        if let Some(menu) = self
            .graphs
            .get(&reference)
            .and_then(|editor| editor.menu.as_ref())
        {
            menu.query
                .update(cx, |state, cx| state.set_value(typed, window, cx));
        }
    }

    /// Drops the editors of tabs that have closed.
    pub(super) fn prune_graph_editors(&mut self) {
        let open = self.scripts.tabs.all().to_vec();
        self.graphs.retain(|reference, _| open.contains(reference));
    }

    /// Finishes a graph edit: compiles the graph and, when that is not what
    /// `Source` holds, writes it (one undo step) and imports it again,
    /// carrying positions, groups and the selection over. A graph that does
    /// not compile is left on screen with its problems and writes nothing.
    /// Compiling to what `Source` already is — a move, a loose node, a
    /// group — keeps the graph as it is.
    fn commit_graph(&mut self, reference: Ref, cx: &mut Context<Self>) {
        // Typing still on its debounce in the Code side belongs to the
        // state of the script before this write, not after it.
        self.flush_script_edits(cx);
        let current = source::read(&self.dom, reference).unwrap_or_default();
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        editor.layout_last = false;
        let Ok(code) = codegen::compile_with(&editor.graph, &editor.origins) else {
            editor.save_layout();
            cx.notify();
            return;
        };
        if code == current {
            editor.built_from = Some(current);
            editor.save_layout();
            cx.notify();
            return;
        }
        self.write_source(reference, &code, cx);
        if let Some(editor) = self.graphs.get_mut(&reference) {
            editor.rebuild(&code);
        }
        cx.notify();
    }

    /// Whether the menu or a literal's field is taking typing, so Ctrl+Z
    /// is that field's and not the place's.
    pub(super) fn graph_field_focused(&self) -> bool {
        self.graphs
            .values()
            .any(|editor| editor.menu.is_some() || editor.literal.is_some())
    }

    /// Writes `code` to the script's `Source` as one undo step.
    fn write_source(&mut self, reference: Ref, code: &str, cx: &mut Context<Self>) {
        self.dom.take_changes();
        let before = self.dom.clone();
        if source::is(&self.dom, reference, code) {
            return;
        }
        source::write(&mut self.dom, reference, code);
        self.push_history_snapshot(before);
        self.properties.dom_changed(&[]);
        let changes = self.dom.take_changes();
        self.record_history_change(changes);
        cx.notify();
    }

    /// The Graph side's body for the front tab.
    pub(super) fn graph_view(
        &mut self,
        reference: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.graph_editor_for(reference, cx);
        self.graph_canvas(reference, window, cx)
    }

    /// Keyboard focus onto a tab's canvas, once it has been drawn.
    pub(super) fn focus_graph(&self, reference: Ref, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.graphs.get(&reference) {
            window.focus(&editor.focus, cx);
        }
    }

    /// The status bar's words while the Graph side is up.
    pub(super) fn graph_status(&self, reference: Ref) -> Option<GraphStatus> {
        let editor = self.graphs.get(&reference)?;
        let graph = &editor.graph;
        let plural = |n: usize, word: &str| match n {
            1 => format!("1 {word}"),
            n => format!("{n} {word}s"),
        };
        let mut parts = vec![plural(graph.nodes.len(), "node")];
        match &editor.gesture {
            Some(Gesture::Wire { end, side, .. }) => {
                let ty = wire_type(graph, end, *side).map_or("", |ty| ty.name());
                let name = match end.pin.is_empty() {
                    true => "the run".to_owned(),
                    false => end.pin.clone(),
                };
                parts.push(format!("dragging a wire from {name} ({ty})"));
            }
            _ => {
                parts.push(plural(graph.wires.len(), "wire"));
                let found = problems(graph, &editor.origins);
                parts.push(match (&editor.notice, found.first()) {
                    (Some(notice), _) => notice.clone(),
                    (None, None) => "no errors".into(),
                    // The first one named: the node at fault is outlined,
                    // but not while it is the one selected.
                    (None, Some(first)) => {
                        format!("{} · {}", plural(found.len(), "error"), first.message)
                    }
                });
                // Legal but easy to miss: a statement no event leads to
                // compiles to nothing.
                match codegen::idle(graph).len() {
                    0 => {}
                    1 => parts.push("1 node never runs".into()),
                    n => parts.push(format!("{n} nodes never run")),
                }
            }
        }
        Some(GraphStatus {
            summary: parts.join(" · "),
            zoom: editor.view.zoom,
        })
    }

    /// The toolbar's tool buttons and zoom read and set these.
    pub(super) fn graph_tool(&self, reference: Ref) -> Option<Tool> {
        self.graphs.get(&reference).map(|editor| editor.tool)
    }

    pub(super) fn set_graph_tool(&mut self, reference: Ref, tool: Tool, cx: &mut Context<Self>) {
        if let Some(editor) = self.graphs.get_mut(&reference) {
            editor.tool = tool;
            cx.notify();
        }
    }
}

fn problems(graph: &Graph, origins: &Origins) -> Vec<Problem> {
    codegen::compile_with(graph, origins)
        .err()
        .unwrap_or_default()
}

/// The type a wire out of `end` carries.
fn wire_type(graph: &Graph, end: &End, side: Side) -> Option<PinType> {
    let pin = match side {
        Side::Input => graph.input_pin(end.node, &end.pin),
        Side::Output => graph.output_pin(end.node, &end.pin),
    }?;
    Some(pin.ty)
}
