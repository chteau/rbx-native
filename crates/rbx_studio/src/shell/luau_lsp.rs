//! The Shell side of `luau-lsp`: starting it the first time a script opens,
//! handing every open tab to it, keeping its copy of the place current, and
//! pulling the diagnostics both the tabs' squiggles and the Script Analysis
//! dock show.
//!
//! One pull feeds both, so a problem is never on one and missing from the
//! other.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::input::RopeExt as _;
use gpui_kit::base::input::ShowDocumentHandler;
use gpui_kit::*;
use lsp_types::{Diagnostic, Range};
use rbx_dom::Ref;
use serde_json::{json, Value};

use crate::luau_lsp::{self, diagnostics, Client, Mirror};
use crate::script_editor::lsp::definition::{self, Definitions};
use crate::script_editor::lsp::{editor_diagnostics, editor_offset, Completions, Document, Hovers};

use super::Shell;

/// How long typing must pause before the place is re-checked. The server
/// rechecks only what changed, but a request per keystroke would still queue
/// work nobody waits to see.
const REFRESH_DELAY: Duration = Duration::from_millis(500);

pub(super) enum Status {
    /// Nothing has asked for it yet.
    Off,
    Starting,
    Ready(Arc<Client>),
    /// Not found, or it failed to start; the reason is shown in the dock.
    Failed(String),
}

pub(super) struct Session {
    pub(super) status: Status,
    mirror: Mirror,
    /// The history revision the mirror last matched.
    synced: Option<u64>,
    /// Every script with a problem, as of the last pull.
    pub(super) problems: Vec<(Ref, Vec<Diagnostic>)>,
    /// Bumped per scheduled refresh; only the latest one runs.
    generation: u64,
    scheduled: bool,
    pub(super) scroll: ScrollHandle,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            status: Status::Off,
            mirror: Mirror::new(luau_lsp::workspace_root()),
            synced: None,
            problems: Vec::new(),
            generation: 0,
            scheduled: false,
            scroll: ScrollHandle::new(),
        }
    }
}

impl Shell {
    /// Starts the server unless it is running, starting or has failed.
    /// Writing the mirror comes first, on this thread, since it reads the
    /// DOM; everything slow happens on a background one.
    pub(super) fn ensure_luau_lsp(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.lsp.status, Status::Off) {
            return;
        }
        if let Err(error) = self.lsp.mirror.sync(&self.dom, &self.database) {
            self.lsp.status =
                Status::Failed(format!("could not write the place for luau-lsp: {error}"));
            return;
        }
        self.lsp.synced = Some(self.history.revision());
        self.lsp.status = Status::Starting;
        let root = self.lsp.mirror.root().to_owned();
        cx.spawn(async move |shell, cx| {
            let started = cx
                .background_executor()
                .spawn(async move { luau_lsp::start(&root) })
                .await;
            let _ = shell.update(cx, |shell, cx| shell.luau_lsp_started(started, cx));
        })
        .detach();
    }

    /// The dock's Retry: forgets a failure and starts over.
    pub(super) fn restart_luau_lsp(&mut self, cx: &mut Context<Self>) {
        self.lsp.status = Status::Off;
        self.ensure_luau_lsp(cx);
        cx.notify();
    }

    fn luau_lsp_started(&mut self, started: Result<Client, String>, cx: &mut Context<Self>) {
        match started {
            Ok(client) => {
                self.lsp.status = Status::Ready(Arc::new(client));
                for reference in self.scripts.tabs.all().to_vec() {
                    self.attach_luau_lsp(reference, cx);
                }
                self.refresh_luau_lsp(cx);
            }
            Err(error) => self.lsp.status = Status::Failed(error),
        }
        cx.notify();
    }

    /// Opens a tab's script on the server and gives its editor completion
    /// and hover. A no-op until the server is up; `luau_lsp_started` comes
    /// back for every tab opened in the meantime.
    pub(super) fn attach_luau_lsp(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Status::Ready(client) = &self.lsp.status else {
            return;
        };
        let Some(open) = self.scripts.open.get_mut(&reference) else {
            return;
        };
        if open.lsp.is_some() {
            return;
        }
        let uri = luau_lsp::uri(&self.lsp.mirror.path_of(reference));
        let text = open.state.read(cx).value().to_string();
        let document = Document::open(client.clone(), uri.clone(), text);
        let shell = cx.weak_entity();
        // The editor's jump for a definition: its own text it jumps within
        // itself; another script is the Shell's to open. Deferred, since this
        // runs inside the editor's own update and the target may be it.
        let show_document: ShowDocumentHandler = Rc::new(move |params, window, cx| {
            let target = params.uri.to_string();
            if definition::same_file(&target, &uri) {
                return false;
            }
            let (shell, selection) = (shell.clone(), params.selection);
            window.defer(cx, move |window, cx| {
                let _ = shell.update(cx, |shell, cx| {
                    shell.reveal_location(&target, selection, window, cx);
                });
            });
            true
        });
        open.state.update(cx, |state, _| {
            let lsp = state.lsp_mut();
            lsp.completion_provider = Some(Rc::new(Completions(document.clone())));
            lsp.hover_provider = Some(Rc::new(Hovers(document.clone())));
            lsp.definition_provider = Some(Rc::new(Definitions(document.clone())));
            lsp.show_document = Some(show_document);
        });
        open.lsp = Some(document);
    }

    /// Re-checks the place once typing pauses. Every call restarts the wait.
    pub(super) fn schedule_luau_lsp_refresh(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.lsp.status, Status::Ready(_)) {
            return;
        }
        self.lsp.generation = self.lsp.generation.wrapping_add(1);
        self.lsp.scheduled = true;
        let generation = self.lsp.generation;
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(REFRESH_DELAY).await;
            let _ = shell.update(cx, |shell, cx| {
                if shell.lsp.generation == generation {
                    shell.refresh_luau_lsp(cx);
                }
            });
        })
        .detach();
    }

    /// Called from a render: schedules a refresh if the DOM moved since the
    /// mirror last matched it — an undo, a Command Bar script, an Argon
    /// sync — and one is not already waiting.
    pub(super) fn notice_luau_lsp_dom_change(&mut self, cx: &mut Context<Self>) {
        if !self.lsp.scheduled && self.lsp.synced != Some(self.history.revision()) {
            self.schedule_luau_lsp_refresh(cx);
        }
    }

    fn refresh_luau_lsp(&mut self, cx: &mut Context<Self>) {
        self.lsp.scheduled = false;
        let Status::Ready(client) = &self.lsp.status else {
            return;
        };
        let client = client.clone();

        let revision = self.history.revision();
        if self.lsp.synced != Some(revision) {
            self.lsp.synced = Some(revision);
            // A failed write leaves the server on the last copy it read,
            // which is stale but still the right place.
            if let Ok(changes) = self.lsp.mirror.sync(&self.dom, &self.database) {
                if !changes.is_empty() {
                    let changes: Vec<Value> = changes
                        .iter()
                        .map(|(path, kind)| json!({"uri": luau_lsp::uri(path), "type": kind}))
                        .collect();
                    client.notify(
                        "workspace/didChangeWatchedFiles",
                        json!({"changes": changes}),
                    );
                }
            }
        }
        for open in self.scripts.open.values() {
            if let Some(document) = &open.lsp {
                document.sync(&open.state.read(cx).value());
            }
        }

        let reply = client.request("workspace/diagnostic", json!({"previousResultIds": []}));
        cx.spawn(async move |shell, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move { luau_lsp::wait(reply) })
                .await;
            let _ = shell.update(cx, |shell, cx| shell.apply_luau_lsp_diagnostics(reply, cx));
        })
        .detach();
    }

    fn apply_luau_lsp_diagnostics(&mut self, reply: Result<Value, String>, cx: &mut Context<Self>) {
        // A failed pull keeps the last list: stale beats blank.
        let Ok(reply) = reply else {
            return;
        };
        self.lsp.problems = diagnostics::parse(&reply, &self.lsp.mirror);
        for (reference, open) in &self.scripts.open {
            let problems = self
                .lsp
                .problems
                .iter()
                .find(|(script, _)| script == reference)
                .map_or(&[][..], |(_, problems)| problems.as_slice());
            let document = open.lsp.clone();
            open.state.update(cx, |state, cx| {
                let text = state.text().clone();
                let moved = editor_diagnostics(&text, problems);
                if let Some(document) = document {
                    let offset = |position| text.position_to_offset(position);
                    document.set_problems(
                        moved
                            .iter()
                            .map(|p| {
                                (
                                    offset(&p.range.start)..offset(&p.range.end),
                                    p.message.clone(),
                                )
                            })
                            .collect(),
                    );
                }
                if let Some(set) = state.diagnostics_mut() {
                    set.reset(&text);
                    set.extend(moved);
                }
                cx.notify();
            });
        }
        cx.notify();
    }

    /// Script Analysis's click: opens the script with the cursor on the
    /// problem.
    pub(super) fn reveal_problem(
        &mut self,
        reference: Ref,
        problem: &Diagnostic,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = problem.range.start;
        self.reveal(reference, Range::new(start, start), window, cx);
    }

    /// A definition in another script: opens it with the name selected.
    /// Anything outside the place — Roblox's own definitions file — has no
    /// tab to open, and is left alone.
    pub(super) fn reveal_location(
        &mut self,
        uri: &str,
        range: Option<Range>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(reference) = luau_lsp::path(uri).and_then(|path| self.lsp.mirror.script_at(&path))
        else {
            return;
        };
        self.reveal(reference, range.unwrap_or_default(), window, cx);
    }

    /// Opens `reference` with `range` — in the server's columns — selected.
    fn reveal(
        &mut self,
        reference: Ref,
        range: Range,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_script(reference, window, cx);
        let Some(open) = self.scripts.open.get(&reference) else {
            return;
        };
        open.state.update(cx, |state, cx| {
            let start = editor_offset(state.text(), range.start);
            let end = editor_offset(state.text(), range.end);
            state.set_selected_range(start..end, cx);
            state.focus(window, cx);
        });
    }

    /// Go to Definition from the right-click menu, asked of the server when
    /// it is up. `false` when it is not, for the caller's lexer lookup.
    pub(super) fn go_to_lsp_definition(
        &mut self,
        reference: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(open) = self.scripts.open.get(&reference) else {
            return false;
        };
        let Some(document) = open.lsp.clone() else {
            return false;
        };
        let (text, offset) = {
            let state = open.state.read(cx);
            (state.text().clone(), state.cursor())
        };
        let reply = document.definition(&text, offset);
        let own = document.uri().to_owned();
        cx.spawn_in(window, async move |shell, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move { luau_lsp::wait(reply) })
                .await;
            let origin = offset..offset;
            let link = reply.ok().and_then(|reply| {
                definition::links(reply, &own, &text, origin)
                    .into_iter()
                    .next()
            });
            let _ = shell.update_in(cx, |shell, window, cx| match link {
                Some(link) if definition::same_file(link.target_uri.as_str(), &own) => {
                    // Already in editor columns; see `definition::links`.
                    if let Some(open) = shell.scripts.open.get(&reference) {
                        open.state.update(cx, |state, cx| {
                            let text = state.text();
                            let start = text.position_to_offset(&link.target_selection_range.start);
                            let end = text.position_to_offset(&link.target_selection_range.end);
                            state.set_selected_range(start..end, cx);
                            state.focus(window, cx);
                        });
                    }
                }
                Some(link) => {
                    let target = link.target_uri.to_string();
                    shell.reveal_location(&target, Some(link.target_selection_range), window, cx);
                }
                None => shell.go_to_lexer_declaration(reference, window, cx),
            });
        })
        .detach();
        true
    }
}
