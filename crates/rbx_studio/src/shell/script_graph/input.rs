//! What a press, drag, wheel or key on the canvas does. A press is tested
//! against `graph::layout` in priority order — pin, literal chip, node,
//! empty canvas — so the small targets win over the node they sit on.
//!
//! Blender's and Unreal's habits where they agree: drag from a pin to draw
//! a wire, drop it on nothing to pick the node it should end on, drag a
//! wired input to pick its wire back up, Shift+A or a double-click to add,
//! Space or the middle button to pan, Ctrl+wheel to zoom.

use std::collections::BTreeSet;

use gpui_kit::*;
use rbx_dom::Ref;

use super::literal::Target;
use super::{canvas, Gesture, Tool};

mod edits;
use crate::script_editor::graph::catalog::Wanted;
use crate::script_editor::graph::layout::{self, Rect, Side};

use super::super::Shell;

impl Shell {
    pub(super) fn graph_input(
        &self,
        element: Stateful<Div>,
        reference: Ref,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        element
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, event: &MouseDownEvent, window, cx| {
                    shell.graph_press(reference, event, window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |shell, event: &MouseDownEvent, window, cx| {
                    let Some(editor) = shell.graphs.get_mut(&reference) else {
                        return;
                    };
                    window.focus(&editor.focus, cx);
                    editor.gesture = Some(Gesture::Pan {
                        panel: editor.panel(event.position),
                    });
                }),
            )
            .on_mouse_move(cx.listener(move |shell, event: &MouseMoveEvent, _, cx| {
                shell.graph_drag(reference, event.position, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |shell, event: &MouseUpEvent, window, cx| {
                    shell.graph_release(reference, event.position, window, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |shell, event: &MouseUpEvent, window, cx| {
                    shell.graph_release(reference, event.position, window, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |shell, event: &MouseUpEvent, window, cx| {
                    shell.graph_release(reference, event.position, window, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(move |shell, event: &MouseUpEvent, window, cx| {
                    shell.graph_release(reference, event.position, window, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(move |shell, event: &ScrollWheelEvent, _, cx| {
                shell.graph_wheel(reference, event, cx);
            }))
            .on_key_down(cx.listener(move |shell, event: &KeyDownEvent, window, cx| {
                if shell.graph_key(reference, &event.keystroke, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_key_up(cx.listener(move |shell, event: &KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" {
                    if let Some(editor) = shell.graphs.get_mut(&reference) {
                        editor.space = false;
                        cx.notify();
                    }
                }
            }))
    }

    fn graph_press(
        &mut self,
        reference: Ref,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_add_menu(reference, cx);
        self.end_literal_edit(reference, true, cx);
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        window.focus(&editor.focus, cx);
        editor.notice = None;
        let panel = editor.panel(event.position);
        let p = editor.view.to_canvas(panel);
        editor.pointer = panel;
        if editor.space || editor.tool == Tool::Hand {
            editor.gesture = Some(Gesture::Pan { panel });
            return;
        }
        let reach = layout::PIN_REACH / editor.view.zoom.clamp(0.5, 1.5);
        let graph = &mut editor.graph;
        if let Some((end, side)) = layout::pin_at(graph, p, reach) {
            // A wired input gives its wire back: picked up from the output
            // end, to be dropped somewhere else or let go of.
            let picked = match side {
                Side::Input => graph.wire_into(&end).map(|wire| wire.from.clone()),
                Side::Output => None,
            };
            editor.gesture = Some(match picked {
                Some(from) => {
                    graph.disconnect(&end);
                    Gesture::Wire {
                        end: from,
                        side: Side::Output,
                        to: p,
                    }
                }
                None => Gesture::Wire { end, side, to: p },
            });
            cx.notify();
            return;
        }
        if let Some(end) = layout::chip_at(graph, p) {
            self.begin_literal_edit(reference, Target::Pin(end), window, cx);
            return;
        }
        if let Some(index) = layout::group_title_at(graph, p) {
            if event.click_count >= 2 {
                self.begin_literal_edit(reference, Target::Group(index), window, cx);
                return;
            }
            let frame = [graph.groups[index].x, graph.groups[index].y];
            editor.group = Some(index);
            editor.selection.clear();
            let origins = graph
                .nodes_within(index)
                .into_iter()
                .filter_map(|id| graph.node(id).map(|node| (id, [node.x, node.y])))
                .collect();
            editor.gesture = Some(Gesture::Move {
                from: p,
                origins,
                group: Some((index, frame)),
                moved: false,
            });
            cx.notify();
            return;
        }
        let handle_reach = layout::HANDLE_REACH / editor.view.zoom.clamp(0.5, 1.5);
        if let Some((index, handle)) = layout::group_handle_at(graph, p, handle_reach) {
            editor.group = Some(index);
            editor.selection.clear();
            editor.gesture = Some(Gesture::Resize {
                index,
                handle,
                from: p,
                origin: graph.groups[index].clone(),
            });
            cx.notify();
            return;
        }
        editor.group = None;
        let shift = event.modifiers.shift;
        if let Some(id) = layout::node_at(graph, p) {
            match (shift, editor.selection.contains(&id)) {
                (true, true) => {
                    editor.selection.remove(&id);
                }
                (true, false) => {
                    editor.selection.insert(id);
                }
                (false, true) => {}
                (false, false) => editor.selection = BTreeSet::from([id]),
            }
            // Brought to the front, so it draws over what it is dragged
            // across and is the one a press finds.
            if let Some(index) = graph.nodes.iter().position(|node| node.id == id) {
                let node = graph.nodes.remove(index);
                graph.nodes.push(node);
            }
            let origins = editor
                .selection
                .iter()
                .filter_map(|id| graph.node(*id).map(|node| (*id, [node.x, node.y])))
                .collect();
            editor.gesture = Some(Gesture::Move {
                from: p,
                origins,
                group: None,
                moved: false,
            });
            cx.notify();
            return;
        }
        if event.click_count >= 2 {
            self.open_add_menu(reference, panel, None, window, cx);
            return;
        }
        let keep = match shift {
            true => editor.selection.clone(),
            false => BTreeSet::new(),
        };
        editor.selection = keep.clone();
        editor.gesture = Some(Gesture::Marquee {
            from: p,
            to: p,
            keep,
        });
        cx.notify();
    }

    fn graph_drag(&mut self, reference: Ref, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        let panel = editor.panel(position);
        editor.pointer = panel;
        let p = editor.view.to_canvas(panel);
        let Some(gesture) = editor.gesture.as_mut() else {
            return;
        };
        match gesture {
            Gesture::Pan { panel: last } => {
                editor.view.pan[0] += panel[0] - last[0];
                editor.view.pan[1] += panel[1] - last[1];
                *last = panel;
                editor.fitted = false;
            }
            Gesture::Move {
                from,
                origins,
                group,
                moved,
            } => {
                let delta = [p[0] - from[0], p[1] - from[1]];
                *moved |= delta[0].abs() + delta[1].abs() > 1.0;
                for (id, origin) in origins.iter() {
                    if let Some(node) = editor.graph.node_mut(*id) {
                        node.x = (origin[0] + delta[0]).round();
                        node.y = (origin[1] + delta[1]).round();
                    }
                }
                if let Some((index, origin)) = group {
                    if let Some(frame) = editor.graph.groups.get_mut(*index) {
                        frame.x = (origin[0] + delta[0]).round();
                        frame.y = (origin[1] + delta[1]).round();
                    }
                }
            }
            Gesture::Resize {
                index,
                handle,
                from,
                origin,
            } => {
                let delta = [p[0] - from[0], p[1] - from[1]];
                if let Some(frame) = editor.graph.groups.get_mut(*index) {
                    *frame = layout::resized(origin, *handle, delta);
                }
            }
            Gesture::Wire { to, .. } => *to = p,
            Gesture::Marquee { from, to, keep } => {
                *to = p;
                let area = Rect::spanning(*from, p);
                let mut selection = keep.clone();
                selection.extend(
                    editor
                        .graph
                        .nodes
                        .iter()
                        .filter(|node| layout::rect(&editor.graph, node).intersects(&area))
                        .map(|node| node.id),
                );
                editor.selection = selection;
            }
        }
        cx.notify();
    }

    fn graph_release(
        &mut self,
        reference: Ref,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        let Some(gesture) = editor.gesture.take() else {
            return;
        };
        let panel = editor.panel(position);
        let p = editor.view.to_canvas(panel);
        match gesture {
            Gesture::Pan { .. } | Gesture::Marquee { .. } => cx.notify(),
            Gesture::Move { moved, .. } => {
                if moved {
                    self.commit_graph(reference, cx);
                }
                cx.notify();
            }
            // commit_graph writes nothing when the frame ended where it began.
            Gesture::Resize { .. } => {
                self.commit_graph(reference, cx);
                cx.notify();
            }
            Gesture::Wire { end, side, .. } => {
                let reach = layout::PIN_REACH / editor.view.zoom.clamp(0.5, 1.5);
                let target = layout::pin_at(&editor.graph, p, reach);
                match target {
                    Some((other, other_side)) if other_side != side => {
                        let (from, to) = match side {
                            Side::Output => (end, other),
                            Side::Input => (other, end),
                        };
                        if let Err(refused) = editor.graph.connect(from, to) {
                            editor.notice = Some(refused.message());
                        }
                        self.commit_graph(reference, cx);
                    }
                    Some(_) => self.commit_graph(reference, cx),
                    None => {
                        let wanted =
                            super::wire_type(&editor.graph, &end, side).map(|ty| match side {
                                Side::Output => Wanted::Input(ty),
                                Side::Input => Wanted::Output(ty),
                            });
                        // A picked-up wire let go of on nothing is gone;
                        // that much is an edit whatever the menu does.
                        self.commit_graph(reference, cx);
                        if let Some(wanted) = wanted {
                            self.open_add_menu(
                                reference,
                                panel,
                                Some((end, side, wanted)),
                                window,
                                cx,
                            );
                        }
                    }
                }
                cx.notify();
            }
        }
    }

    /// Keys with the canvas focused. Undo and redo are the window's, and
    /// reach the graph back through its attribute.
    fn graph_key(
        &mut self,
        reference: Ref,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return false;
        };
        let m = keystroke.modifiers;
        match keystroke.key.as_str() {
            "space" => {
                editor.space = true;
                true
            }
            "a" if m.shift && !m.secondary() => {
                let at = editor.pointer;
                self.open_add_menu(reference, at, None, window, cx);
                true
            }
            "a" if m.secondary() => {
                editor.selection = editor.graph.nodes.iter().map(|node| node.id).collect();
                cx.notify();
                true
            }
            "delete" | "backspace" if editor.group.is_some() => {
                if let Some(index) = editor.group.take() {
                    editor.graph.groups.remove(index);
                }
                self.commit_graph(reference, cx);
                true
            }
            "delete" | "backspace" if !editor.selection.is_empty() => {
                let doomed = std::mem::take(&mut editor.selection);
                editor.graph.remove(&doomed);
                self.commit_graph(reference, cx);
                true
            }
            "d" if m.secondary() && !editor.selection.is_empty() => {
                self.duplicate_graph_selection(reference, cx);
                true
            }
            "g" if m.secondary() && !editor.selection.is_empty() => {
                self.group_graph_selection(reference, cx);
                true
            }
            "f" if !m.secondary() => {
                canvas::fit_now(editor);
                cx.notify();
                true
            }
            "escape" => {
                editor.gesture = None;
                editor.selection.clear();
                editor.group = None;
                cx.notify();
                true
            }
            _ => false,
        }
    }
}
