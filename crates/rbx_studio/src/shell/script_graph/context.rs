//! The canvas's right-click menu: on a node, its own commands (add or remove
//! a repeating input, duplicate, delete); on empty canvas, add a node,
//! Optimize graph and fit.

use gpui_kit::*;
use rbx_dom::Ref;

use super::canvas;
use crate::script_editor::graph::layout;
use crate::script_editor::graph::sync;
use crate::script_editor::graph::NodeId;
use crate::tokens;

use super::super::Shell;

const WIDTH: f32 = 190.0;
const ROW_HEIGHT: f32 = 26.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    AddNode,
    Optimize,
    Fit,
    AddInput(NodeId),
    RemoveInput(NodeId),
    Duplicate,
    Delete,
}

pub(super) struct ContextMenu {
    /// Where it opened, in panel pixels.
    panel: [f32; 2],
    /// The node it was opened on.
    node: Option<NodeId>,
}

impl Shell {
    /// Opens the menu at `panel`; a node under it joins the selection.
    pub(super) fn open_context_menu(
        &mut self,
        reference: Ref,
        panel: [f32; 2],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_add_menu(reference, cx);
        self.end_literal_edit(reference, true, cx);
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        window.focus(&editor.focus, cx);
        let p = editor.view.to_canvas(panel);
        let node = layout::node_at(&editor.graph, p);
        if let Some(id) = node {
            if !editor.selection.contains(&id) {
                editor.selection = [id].into();
            }
            editor.group = None;
        }
        editor.context = Some(ContextMenu { panel, node });
        cx.notify();
    }

    pub(super) fn close_context_menu(&mut self, reference: Ref, cx: &mut Context<Self>) {
        if let Some(editor) = self.graphs.get_mut(&reference) {
            if editor.context.take().is_some() {
                cx.notify();
            }
        }
    }

    fn run_context(
        &mut self,
        reference: Ref,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        let at = editor.context.take().map(|menu| menu.panel);
        match action {
            Action::AddNode => {
                let at = at.unwrap_or(editor.pointer);
                self.open_add_menu(reference, at, None, window, cx);
            }
            Action::Optimize => self.optimize_graph(reference, cx),
            Action::Fit => canvas::fit_now(editor),
            Action::AddInput(id) | Action::RemoveInput(id) => {
                let grow = matches!(action, Action::AddInput(_));
                if sync::resize_repeat(&mut editor.graph, id, grow) {
                    self.commit_graph(reference, cx);
                }
            }
            Action::Duplicate => self.duplicate_graph_selection(reference, cx),
            Action::Delete => {
                let doomed = std::mem::take(&mut editor.selection);
                editor.graph.remove(&doomed);
                self.commit_graph(reference, cx);
            }
        }
        window.focus(&self.graphs[&reference].focus, cx);
        cx.notify();
    }

    pub(super) fn context_menu_element(
        &mut self,
        reference: Ref,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editor = self.graphs.get(&reference)?;
        let menu = editor.context.as_ref()?;
        let mut items: Vec<(&'static str, Action)> = Vec::new();
        match menu.node {
            Some(id) => {
                if sync::can_resize(&editor.graph, id, true) {
                    items.push(("Add input", Action::AddInput(id)));
                }
                if sync::can_resize(&editor.graph, id, false) {
                    items.push(("Remove input", Action::RemoveInput(id)));
                }
                items.push(("Duplicate", Action::Duplicate));
                items.push(("Delete", Action::Delete));
            }
            None => {
                items.push(("Add node", Action::AddNode));
                items.push(("Optimize graph", Action::Optimize));
                items.push(("Fit the graph", Action::Fit));
            }
        }
        let panel = editor.panel_size();
        let height = items.len() as f32 * ROW_HEIGHT + 8.0;
        let left = menu.panel[0].min(panel[0] - WIDTH - 8.0).max(8.0);
        let top = menu.panel[1].min(panel[1] - height - 8.0).max(8.0);
        let count = items.len();
        let rows: Vec<AnyElement> = items
            .into_iter()
            .enumerate()
            .map(|(index, (label, action))| {
                div()
                    .id(("graph-context", index))
                    .role(Role::MenuItem)
                    .aria_label(label)
                    .aria_position_in_set(index + 1)
                    .aria_size_of_set(count)
                    .h(px(ROW_HEIGHT))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(tokens::radius())
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text())
                    .cursor_pointer()
                    .hover(|row| row.bg(tokens::selection()))
                    .child(label)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |shell, _, window, cx| {
                            cx.stop_propagation();
                            shell.run_context(reference, action, window, cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .id("graph-context-menu")
                .role(Role::Menu)
                .aria_label("Graph commands")
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(WIDTH))
                .p(px(4.0))
                .bg(tokens::tile())
                .border_1()
                .border_color(tokens::border())
                .rounded(tokens::radius())
                .shadow_lg()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .children(rows)
                .into_any_element(),
        )
    }
}
