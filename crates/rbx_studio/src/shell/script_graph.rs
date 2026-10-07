//! The Script Editor's Graph side: one [`GraphEditor`] per open tab that
//! has shown it, drawing and editing the graph `script_editor::graph`
//! defines.
//!
//! The DOM stays the only copy that counts, as it is for code. The graph is
//! read from the script's attribute whenever that text moves (an undo, a
//! sync), and every finished edit — a drop, a wire, a typed literal —
//! writes the attribute and, when the graph compiles, `Source`, as one undo
//! step. A drag in flight edits only the copy here, so a node dragged
//! across the canvas is one step, not one per frame.

mod add_menu;
mod canvas;
mod input;
mod literal;
mod nodes;
mod style;

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use gpui_kit::*;
use rbx_dom::{Ref, Variant};

use crate::properties::attributes;
use crate::script_editor::graph::catalog::{PinType, Wanted};
use crate::script_editor::graph::codegen::{self, Problem};
use crate::script_editor::graph::layout::Side;
use crate::script_editor::graph::{End, Graph, NodeId, ATTRIBUTE};
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
}

pub(super) struct GraphEditor {
    graph: Graph,
    /// The attribute text `graph` was last read from or written as.
    synced: Option<String>,
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
            synced: None,
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
}

/// What the status bar reads for the Graph side.
pub(super) struct GraphStatus {
    pub(super) summary: String,
    pub(super) zoom: f32,
}

impl Shell {
    /// The editor for `reference`, made on first use and kept in step with
    /// the DOM's attribute.
    fn graph_editor_for(&mut self, reference: Ref, cx: &mut App) -> &mut GraphEditor {
        let text = graph_text(&self.dom, reference);
        let editor = self
            .graphs
            .entry(reference)
            .or_insert_with(|| GraphEditor::new(cx));
        if editor.gesture.is_none() && editor.synced != text {
            editor.graph = text.as_deref().and_then(Graph::parse).unwrap_or_default();
            editor
                .selection
                .retain(|id| editor.graph.node(*id).is_some());
            editor.group = editor.group.filter(|&i| i < editor.graph.groups.len());
            editor.synced = text;
        }
        editor
    }

    /// `RBX_STUDIO_SCRIPT_VIEW=graph=<file>`: the graph in `path` saved as
    /// `reference`'s, through the canvas's own commit. A file that does not
    /// hold a graph is ignored.
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
        self.graph_editor_for(reference, cx).graph = graph;
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
                let kind = graph.kind_of(node.id)?;
                let pin = kind.outputs.iter().find(|pin| pin.ty != PinType::Exec)?;
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

    /// Writes the editor's graph to the DOM if it changed: the attribute,
    /// and `Source` when it compiles. One undo step.
    fn commit_graph(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(editor) = self.graphs.get(&reference) else {
            return;
        };
        let json = editor.graph.to_json();
        if editor.synced.as_deref() == Some(json.as_str()) {
            return;
        }
        let code = codegen::compile(&editor.graph).ok();
        // Typing still on its debounce in the Code side belongs to the
        // state of the script before this write, not after it.
        self.flush_script_edits(cx);
        self.write_graph(reference, Some(json), code, cx);
    }

    /// Replaces the script's code with what its graph compiles to, for a
    /// script whose code was edited since its graph was last saved.
    fn graph_to_code(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(code) = self
            .graphs
            .get(&reference)
            .and_then(|editor| codegen::compile(&editor.graph).ok())
        else {
            return;
        };
        self.flush_script_edits(cx);
        self.write_graph(reference, None, Some(code), cx);
    }

    fn write_graph(
        &mut self,
        reference: Ref,
        json: Option<String>,
        code: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.dom.take_changes();
        let before = self.dom.clone();
        if let Some(json) = &json {
            let value = Some(Variant::String(json.clone()));
            if attributes::put_attribute(&mut self.dom, reference, ATTRIBUTE, value).is_err() {
                return;
            }
        }
        if let Some(code) = code.filter(|code| !source::is(&self.dom, reference, code)) {
            source::write(&mut self.dom, reference, &code);
        }
        self.push_history_snapshot(before);
        self.properties.dom_changed(&[]);
        let changes = self.dom.take_changes();
        self.record_history_change(changes);
        if let (Some(json), Some(editor)) = (json, self.graphs.get_mut(&reference)) {
            editor.synced = Some(json);
        }
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
                let found = problems(graph);
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

/// The attribute's text, when the script has one.
fn graph_text(dom: &rbx_dom::WeakDom, reference: Ref) -> Option<String> {
    match attributes::attributes(dom, reference).remove(ATTRIBUTE) {
        Some(Variant::String(text)) => Some(text),
        _ => None,
    }
}

fn problems(graph: &Graph) -> Vec<Problem> {
    codegen::compile(graph).err().unwrap_or_default()
}

/// The type a wire out of `end` carries.
fn wire_type(graph: &Graph, end: &End, side: Side) -> Option<PinType> {
    let kind = graph.kind_of(end.node)?;
    let pin = match side {
        Side::Input => kind.input(&end.pin),
        Side::Output => kind.output(&end.pin),
    }?;
    Some(pin.ty)
}
