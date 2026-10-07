//! "Import from Figma…": a window of its own (see `window`) that signs in
//! to Figma (OAuth 2 with PKCE, see `rbx_figma::oauth`), browses recent
//! files and their node trees, and reviews the tree inferred from a frame
//! (see `rbx_figma::infer`) before anything is uploaded. Only Import
//! changes the place: the reviewed tree goes under the canvas's
//! `ScreenGui` as one undo step, its pictures uploaded through the Open
//! Cloud key first.
//!
//! `RBX_STUDIO_FIGMA=home|file|review` opens the window with the editor,
//! for a capture; see `window` for the rest of the capture variables and
//! `source` for `RBX_STUDIO_FIGMA_FIXTURE`.

use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_figma::infer::Node;
use rbx_reflection::ReflectionDatabase;

use super::Shell;
use crate::command_bar::Feedback;
use crate::explorer;

mod model;
mod source;
mod view;
mod window;

const SOURCE: &str = "Figma import";

impl Shell {
    /// Brings the Figma window forward, opening it if it isn't.
    pub(super) fn open_figma(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.ui.figma {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        // Deferred: the window's first render reads this `Shell`, which is
        // still being updated here.
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = window::FigmaWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.ui.figma = opened);
        });
    }

    /// `RBX_STUDIO_FIGMA`; see the module doc.
    pub(in crate::shell) fn apply_debug_figma(&mut self, cx: &mut Context<Self>) {
        if std::env::var_os(window::IMPORT_VARIABLE).is_some() {
            // A screen of its own, drawn edge to edge like Figma's frame.
            self.insert_on_canvas("ScreenGui", cx);
            if let Some(screen) = self.canvas_request().map(|request| request.screen) {
                let _ =
                    self.dom
                        .set_property(screen, "IgnoreGuiInset", rbx_dom::Variant::Bool(true));
            }
        }
        if std::env::var_os(window::OPEN_VARIABLE).is_some()
            || std::env::var_os(window::IMPORT_VARIABLE).is_some()
        {
            self.open_figma(cx);
        }
    }

    /// An import's line in the Output dock and the Command Bar.
    fn figma_feedback(&mut self, line: &Result<String, String>, cx: &mut Context<Self>) {
        let feedback = match line {
            Ok(text) => Feedback::Output(text.clone()),
            Err(text) => Feedback::Error(text.clone()),
        };
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }

    /// The whole tree under the canvas's `ScreenGui`, as one undo step.
    fn insert_figma_tree(&mut self, tree: Node, cx: &mut Context<Self>) -> Result<String, String> {
        let screen = self
            .canvas_request()
            .map(|request| request.screen)
            .filter(|&screen| self.dom.get(screen).is_some())
            .ok_or("Open a ScreenGui on the canvas first.")?;
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let root = materialize(&mut dom, &self.database, screen, tree);
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.select(root, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        Ok("Imported from Figma.".into())
    }
}

/// The root's size in pixels, for a parity import's canvas.
fn frame_size(tree: &Node) -> Option<(u32, u32)> {
    tree.properties
        .iter()
        .find_map(|(name, value)| match (name, value) {
            (&"Size", rbx_dom::Variant::UDim2(size)) => {
                Some((size.x.offset.max(1) as u32, size.y.offset.max(1) as u32))
            }
            _ => None,
        })
}

/// Whether `class` (or a class it inherits) has `property`. A class the
/// database doesn't know keeps everything.
fn class_has(database: &ReflectionDatabase, class: &str, property: &str) -> bool {
    database.class(class).is_none()
        || database
            .resolve_property(class, database.canonical_name(class, property))
            .is_some()
}

/// Builds `node` under `parent`: the editor's own GUI seeds first, then
/// what the design says, minus what its class lacks (review can swap a
/// `TextLabel` for a `Frame`, which has no `Text`).
fn materialize(dom: &mut WeakDom, database: &ReflectionDatabase, parent: Ref, node: Node) -> Ref {
    let reference = dom.new_instance(node.class, &node.name, Some(parent));
    for (name, text) in explorer::insert::gui_defaults(database, node.class) {
        let _ = crate::properties::edit::commit(dom, database, reference, name, text);
    }
    for (name, value) in node.properties {
        if class_has(database, node.class, name) {
            let _ = dom.set_property(reference, name, value);
        }
    }
    for child in node.children {
        materialize(dom, database, reference, child);
    }
    reference
}

#[cfg(test)]
mod tests {
    use super::{class_has, materialize};
    use rbx_dom::{Variant, WeakDom};
    use rbx_figma::infer::Node;
    use rbx_reflection::ReflectionDatabase;

    #[test]
    fn properties_are_checked_up_the_class_chain() {
        let database = ReflectionDatabase::embedded();
        assert!(class_has(&database, "TextLabel", "Text"));
        assert!(class_has(&database, "TextLabel", "BackgroundColor3"));
        assert!(!class_has(&database, "Frame", "Text"));
        assert!(class_has(&database, "NotAClass", "Anything"));
    }

    #[test]
    fn a_changed_class_drops_what_it_lacks() {
        let database = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let screen = dom.new_instance("ScreenGui", "Screen", None);
        let tree = rbx_figma::infer::infer(&serde_json::json!({
            "id": "1:1", "type": "TEXT", "name": "Title", "characters": "Shop",
            "absoluteBoundingBox": { "x": 0, "y": 0, "width": 80, "height": 20 },
            "style": { "fontFamily": "Inter", "fontSize": 18 },
        }))
        .unwrap();
        let mut frame: Node = tree.clone();
        frame.set_class("Frame");
        let made = materialize(&mut dom, &database, screen, frame);
        let made = dom.get(made).unwrap();
        assert_eq!(made.class(), "Frame");
        assert!(!made.properties().contains_key("Text"));
        assert!(matches!(
            made.properties().get("Size"),
            Some(Variant::UDim2(_))
        ));
    }
}
