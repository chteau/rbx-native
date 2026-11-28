//! Sync Shared: the active place's shared assets against every other place
//! holding the same ids, with each asset's diff (in the Argon diff window,
//! read-only) and an Apply switch per place.
//!
//! The source is whichever place was active when the window opened or was
//! last refreshed. Applying replaces each target's copy by the source's,
//! as one undo step of that target place, loading it first if it was not
//! opened yet.

use std::rc::Rc;

use gpui_kit::*;
use rbx_dom::diff::{ChangeKind, DiffNode};
use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use crate::tokens;

use super::super::Shell;
use super::shared::{self, Pair};
use super::workspace::GameWorkspace;

mod view;

/// One other place of the game, against the source.
pub(super) struct Target {
    pub(super) slot: usize,
    pub(super) label: String,
    pub(super) from_file: bool,
    pub(super) pairs: Vec<Pair>,
    /// Whether Apply writes this place; on by default when anything differs.
    pub(super) apply: bool,
}

impl Target {
    pub(super) fn changed(&self) -> bool {
        self.pairs.iter().any(|pair| !pair.nodes.is_empty())
    }
}

pub(crate) struct SyncWindow {
    game: WeakEntity<GameWorkspace>,
    main: AnyWindowHandle,
    database: ReflectionDatabase,
    pub(super) source_label: String,
    /// The source's sync roots: `(id, name)`.
    pub(super) assets: Vec<(String, String)>,
    pub(super) targets: Vec<Target>,
    pub(super) notice: Option<String>,
    pub(super) scroll: ScrollHandle,
}

impl GameWorkspace {
    pub(crate) fn open_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.sync_window {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        let Some(database) = self
            .active_shell()
            .map(|shell| shell.read(cx).database.clone())
        else {
            return;
        };
        let game = cx.weak_entity();
        let main = window.window_handle();
        let title = format!("Sync Shared \u{2014} {}", self.manifest.name);
        cx.defer(move |cx| {
            let opened = super::secondary_window(title, (780., 600.), cx, {
                let game = game.clone();
                move |_, cx| {
                    let mut sync = SyncWindow {
                        game,
                        main,
                        database,
                        source_label: String::new(),
                        assets: Vec::new(),
                        targets: Vec::new(),
                        notice: None,
                        scroll: ScrollHandle::new(),
                    };
                    sync.refresh(cx);
                    sync
                }
            });
            let _ = game.update(cx, |game, _| game.sync_window = opened);
        });
    }
}

impl SyncWindow {
    /// Re-reads the source (the active place) and every other place.
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.targets.clear();
        self.assets.clear();
        let Some(game) = self.game.upgrade() else {
            return;
        };
        let game = game.read(cx);
        let Some(source) = game.active_shell() else {
            return;
        };
        let source = &source.read(cx).dom;
        self.source_label = game.slots[game.active].label.clone();
        self.assets = shared::shared_roots(source)
            .into_iter()
            .filter_map(|(id, reference)| Some((id, source.get(reference)?.name().to_owned())))
            .collect();
        let mut failures = Vec::new();
        for index in (0..game.slots.len()).filter(|&index| index != game.active) {
            let label = game.slots[index].label.clone();
            let (pairs, from_file) = match game.shell(index) {
                Some(shell) => (
                    shared::plan(source, &shell.read(cx).dom, &self.database),
                    false,
                ),
                None => match rbx_viewer::read_place(&game.slots[index].path) {
                    Ok(dom) => (shared::plan(source, &dom, &self.database), true),
                    Err(err) => {
                        failures.push(format!("{label}: {err}"));
                        continue;
                    }
                },
            };
            let mut target = Target {
                slot: index,
                label,
                from_file,
                pairs,
                apply: false,
            };
            target.apply = target.changed();
            self.targets.push(target);
        }
        self.notice =
            (!failures.is_empty()).then(|| format!("Couldn\u{2019}t read {}", failures.join("; ")));
        cx.notify();
    }

    pub(super) fn toggle(&mut self, slot: usize, cx: &mut Context<Self>) {
        if let Some(target) = self.targets.iter_mut().find(|t| t.slot == slot) {
            target.apply = !target.apply && target.changed();
            cx.notify();
        }
    }

    /// One asset's diff in the Argon diff window, read-only.
    pub(super) fn view_diff(&self, slot: usize, pair: usize, cx: &mut Context<Self>) {
        let Some(game) = self.game.upgrade() else {
            return;
        };
        let Some(target) = self.targets.iter().find(|t| t.slot == slot) else {
            return;
        };
        let Some(pair) = target.pairs.get(pair) else {
            return;
        };
        let Some(shell) = game.read(cx).active_shell().cloned() else {
            return;
        };
        let title = format!(
            "{} \u{2014} {} \u{2192} {}",
            pair.name, self.source_label, target.label
        );
        let nodes = Rc::new(pair.nodes.clone());
        cx.defer(move |cx| {
            super::super::argon_diff_window::ArgonDiffWindow::open_shared(shell, nodes, title, cx);
        });
    }

    /// Applies the source's assets to every place switched on.
    pub(super) fn apply(&mut self, cx: &mut Context<Self>) {
        let slots: Vec<usize> = self
            .targets
            .iter()
            .filter(|target| target.apply)
            .map(|target| target.slot)
            .collect();
        let game = self.game.clone();
        let outcome = self.main.update(cx, |_, window, cx| {
            game.update(cx, |game, cx| {
                let source = game.active_shell()?.read(cx).dom.clone();
                let mut report = Vec::new();
                for slot in slots {
                    let label = game.slots[slot].label.clone();
                    match game.load(slot, window, cx) {
                        Ok(shell) => {
                            let count = shell
                                .update(cx, |shell, cx| shell.apply_shared(&source, window, cx));
                            report.push(format!("{label}: {count} updated"));
                        }
                        Err(err) => report.push(format!("{label}: couldn\u{2019}t open ({err})")),
                    }
                }
                cx.notify();
                Some(report)
            })
        });
        self.refresh(cx);
        self.notice = Some(match outcome {
            Ok(Ok(Some(report))) if report.is_empty() => "Nothing to apply.".to_owned(),
            Ok(Ok(Some(report))) => format!(
                "Applied. {}. Ctrl+Z in a place undoes its part.",
                report.join(", ")
            ),
            _ => "The game window is gone.".to_owned(),
        });
    }
}

/// `(updated, added, removed)` instance counts in `nodes`, descendants of an
/// added or removed subtree included.
pub(super) fn counts(nodes: &[DiffNode]) -> (usize, usize, usize) {
    nodes
        .iter()
        .fold((0, 0, 0), |(u, a, r), node| match node.kind {
            ChangeKind::Updated => (u + 1, a, r),
            ChangeKind::Added => (u, a + 1 + node.nested, r),
            ChangeKind::Removed => (u, a, r + 1 + node.nested),
        })
}

impl Shell {
    /// Edit › Mark as Shared: gives each selected Model, Folder
    /// or ModuleScript without an id a fresh one, as one undo step.
    pub(crate) fn mark_shared(&mut self, cx: &mut Context<Self>) {
        let marked: Vec<_> = self
            .selected_all()
            .iter()
            .copied()
            .filter(|&r| {
                shared::shareable(&self.dom, &self.database, r)
                    && shared::shared_id(&self.dom, r).is_none()
            })
            .collect();
        if marked.is_empty() {
            self.warn(
                "Mark as Shared: select a Model, Folder or ModuleScript that is not shared yet."
                    .into(),
                cx,
            );
            return;
        }
        self.push_history();
        for &reference in &marked {
            let _ = shared::mark(&mut self.dom, reference, &shared::new_id());
        }
        self.finish_shared_edit(cx);
        self.output.push(
            "Game",
            crate::command_bar::Feedback::Info(format!(
                "Marked {} as shared ({} attribute). Sync Shared copies it to every place holding the same id.",
                plural(marked.len()),
                shared::SHARED_ATTRIBUTE
            )),
        );
    }

    /// Model › Unmark Shared: drops the id from each selected instance.
    pub(crate) fn unmark_shared(&mut self, cx: &mut Context<Self>) {
        let marked: Vec<_> = self
            .selected_all()
            .iter()
            .copied()
            .filter(|&r| shared::shared_id(&self.dom, r).is_some())
            .collect();
        if marked.is_empty() {
            return;
        }
        self.push_history();
        for &reference in &marked {
            let _ = shared::unmark(&mut self.dom, reference);
        }
        self.finish_shared_edit(cx);
    }

    fn finish_shared_edit(&mut self, cx: &mut Context<Self>) {
        let changes = self.dom.take_changes();
        self.properties.dom_changed(&[]);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }

    /// Replaces this place's copies of `source`'s shared assets, as one undo
    /// step; returns how many were replaced.
    fn apply_shared(
        &mut self,
        source: &WeakDom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        self.flush_script_edits(cx);
        let pairs = shared::plan(source, &self.dom, &self.database);
        if pairs.iter().all(|pair| pair.nodes.is_empty()) {
            return 0;
        }
        self.push_history();
        let replaced = shared::apply(&mut self.dom, source, &pairs);
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        let kept: Vec<_> = self
            .selected_all()
            .iter()
            .copied()
            .filter(|&r| self.dom.get(r).is_some())
            .collect();
        self.reselect(kept, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.resync_scripts(window, cx);
        cx.notify();
        replaced.len()
    }
}

fn plural(count: usize) -> String {
    match count {
        1 => "1 instance".to_owned(),
        n => format!("{n} instances"),
    }
}

impl Render for SyncWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::component::v_flex()
            .id("sync-shared")
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .text_color(tokens::text())
            .child(super::super::chrome::window_topbar(
                "Sync Shared".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(self.header(cx))
            .child(self.list(cx))
            .child(self.footer(cx))
    }
}
