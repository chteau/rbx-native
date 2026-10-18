//! File › Merge Place…: merges another copy of this place into the open
//! one, three-way against their common ancestor (see `rbx_dom::merge`),
//! and the hand-off from the `rbxmerge` git driver: a place opened with a
//! `<file>.merge.json` beside it reopens the conflicts that driver left.
//!
//! A merge with no conflicts lands at once; otherwise the Argon diff window
//! opens in its merge mode (see `argon_diff_window::merge`), and Apply lays
//! the resolved output over the place as one undoable edit.

mod sidecar;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::*;
use rbx_dom::merge::{Merge, Side, Target};
use rbx_dom::{Ref, WeakDom};

use super::argon_sync::{ChangeKind, DiffNode, PropertyChange};
use super::Shell;
use crate::command_bar::Feedback;

/// A merge the window is resolving.
pub(crate) struct MergeSession {
    pub(crate) merge: Merge,
    /// What the window lists: one `ChangeKind::Updated` node per conflicted
    /// instance (first-conflict order; `id` = index; `name`/`class` from the
    /// conflict; `path` = the parent's full name, `game` for a root), with
    /// one `PropertyChange` per conflict (`name` = [`target_label`],
    /// `before` = ours, `after` = theirs) and no `source` or children.
    pub(crate) nodes: Rc<Vec<DiffNode>>,
    /// Conflict ids (`Merge::conflicts` indices) per node id.
    pub(crate) by_node: Vec<Vec<usize>>,
    /// The driver's sidecar this merge reopened; deleted once applied.
    pub(crate) sidecar: Option<PathBuf>,
    /// Where "theirs" came from, for the window's subtitle (a file name).
    pub(crate) theirs_label: String,
}

impl MergeSession {
    pub(crate) fn new(merge: Merge, sidecar: Option<PathBuf>, theirs_label: String) -> Self {
        let mut nodes: Vec<DiffNode> = Vec::new();
        let mut by_node: Vec<Vec<usize>> = Vec::new();
        let mut node_of: HashMap<Ref, usize> = HashMap::new();
        for conflict in merge.conflicts() {
            let index = *node_of.entry(conflict.instance).or_insert_with(|| {
                let path = conflict
                    .path
                    .rsplit_once('.')
                    .map_or("game", |(parent, _)| parent)
                    .to_string();
                nodes.push(DiffNode {
                    id: nodes.len(),
                    kind: ChangeKind::Updated,
                    name: conflict.name.clone(),
                    class: conflict.class.clone(),
                    path,
                    properties: Vec::new(),
                    source: None,
                    children: Vec::new(),
                    nested: 0,
                    contents: Vec::new(),
                });
                by_node.push(Vec::new());
                nodes.len() - 1
            });
            nodes[index].properties.push(PropertyChange {
                name: target_label(&conflict.target),
                before: conflict.ours.clone(),
                after: conflict.theirs.clone(),
            });
            by_node[index].push(conflict.id);
        }
        MergeSession {
            merge,
            nodes: Rc::new(nodes),
            by_node,
            sidecar,
            theirs_label,
        }
    }
}

/// A conflict's row label: `Name`, `ClassName`, the property's name,
/// `Attribute <name>`, `Source hunk <n+1>`, `Parent`, `Removed by us` /
/// `Removed by them`.
pub(crate) fn target_label(target: &Target) -> String {
    match target {
        Target::Name => "Name".to_string(),
        Target::Class => "ClassName".to_string(),
        Target::Property(name) => name.clone(),
        Target::Attribute(name) => format!("Attribute {name}"),
        Target::Source(n) => format!("Source hunk {}", n + 1),
        Target::Parent => "Parent".to_string(),
        Target::Existence {
            removed_by: Side::Ours,
        } => "Removed by us".to_string(),
        Target::Existence { .. } => "Removed by them".to_string(),
    }
}

fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

impl Shell {
    /// File › Merge Place…: asks for the common base file, then for theirs.
    pub(crate) fn open_merge_place(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Merge Place: common base".into()),
        });
        cx.spawn(async move |shell, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(base_path) = paths.into_iter().next() else {
                return;
            };
            let _ = shell.update(cx, |_, cx| {
                let prompt = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some("Merge Place: their version".into()),
                });
                cx.spawn(async move |shell, cx| {
                    let Ok(Ok(Some(paths))) = prompt.await else {
                        return;
                    };
                    let Some(theirs_path) = paths.into_iter().next() else {
                        return;
                    };
                    let _ = shell.update(cx, |shell, cx| {
                        let read = rbx_viewer::read_place(&base_path)
                            .and_then(|base| Ok((base, rbx_viewer::read_place(&theirs_path)?)));
                        match read {
                            Ok((base, theirs)) => {
                                shell.merge_with(base, theirs, None, file_label(&theirs_path), cx)
                            }
                            Err(e) => shell
                                .output
                                .push("Merge", Feedback::Error(format!("Merge Place: {e}"))),
                        }
                    });
                })
                .detach();
            });
        })
        .detach();
    }

    /// At startup: the `rbxmerge` sidecar beside the open file, if any,
    /// reopened as a merge (Output says so either way it goes).
    pub(super) fn offer_merge_sidecar(&mut self, cx: &mut Context<Self>) {
        let path = sidecar::path_for(&self.path);
        if !path.exists() {
            return;
        }
        match sidecar::read(&path) {
            Ok(found) => {
                let label = found.file.clone();
                self.merge_with(found.base, found.theirs, Some(found.path), label, cx);
            }
            Err(e) => self.output.push("Merge", Feedback::Warning(e)),
        }
    }

    fn merge_with(
        &mut self,
        base: WeakDom,
        theirs: WeakDom,
        sidecar: Option<PathBuf>,
        label: String,
        cx: &mut Context<Self>,
    ) {
        let merge = Merge::new(base, self.dom.clone(), theirs);
        let conflicts = merge.conflicts().len();
        let session = MergeSession::new(merge, sidecar, label.clone());
        if conflicts == 0 {
            self.apply_place_merge(&session, cx);
            self.output.push(
                "Merge",
                Feedback::Info(format!("Merged {label} with no conflicts.")),
            );
            return;
        }
        self.output.push(
            "Merge",
            Feedback::Info(format!("{conflicts} conflicts merging {label}")),
        );
        let shell = cx.entity();
        cx.defer(move |cx| {
            let _ =
                crate::shell::argon_diff_window::ArgonDiffWindow::open_merge(shell, session, cx);
        });
    }

    /// Lays `session`'s output over the place as one undoable edit, then
    /// deletes its sidecar.
    pub(crate) fn apply_place_merge(&mut self, session: &MergeSession, cx: &mut Context<Self>) {
        self.push_history();
        rbx_dom::merge::apply(&mut self.dom, session.merge.ours(), &session.merge.output());
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if let Some(path) = &session.sidecar {
            if let Err(e) = std::fs::remove_file(path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    self.output.push(
                        "Merge",
                        Feedback::Warning(format!("Could not delete {}: {e}", path.display())),
                    );
                }
            }
        }
        self.output
            .push("Merge", Feedback::Info("Merge applied.".to_string()));
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
