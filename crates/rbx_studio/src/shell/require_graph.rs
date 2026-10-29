//! View › Require Graph, behind the `require-graph` beta switch: every
//! script and `ModuleScript` as a box, an edge for each `require(...)` that
//! `rbx_audit::require_graph` resolves, and beside the graph what it finds
//! wrong: cycles, client scripts requiring server-only modules, requires no
//! path can be followed for statically, and modules nothing requires.
//! Clicking a box opens its script; clicking an edge's line badge (or an
//! unknown require's box) opens the requiring script at the call.
//!
//! The window keeps no copy of the place. It graphs a slim copy
//! (`rbx_audit::slim_copy`) off the UI thread, again once edits have been
//! quiet for [`QUIET`], the rhythm the Security Audit dock keeps.
//!
//! `RBX_STUDIO_REQUIRE_GRAPH=1` opens it with the editor (with the beta on),
//! for a capture.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex, Root};
use gpui_kit::*;
use rbx_audit::{RequireGraph, Target};
use rbx_dom::Ref;

use crate::launcher::ui;
use crate::tokens;

use super::Shell;

mod issues;
mod layout;
mod view;

pub(crate) const BETA_ID: &str = "require-graph";
/// Opens the window with the editor, for a capture.
pub(crate) const OPEN_VARIABLE: &str = "RBX_STUDIO_REQUIRE_GRAPH";

/// How long the place has to stay unchanged before an edit re-graphs it.
const QUIET: Duration = Duration::from_millis(300);
const WIDTH: f32 = 1200.;
const HEIGHT: f32 = 760.;
const MIN_WIDTH: f32 = 720.;
const MIN_HEIGHT: f32 = 420.;

pub(crate) struct RequireGraphWindow {
    shell: Entity<Shell>,
    graph: Option<Arc<RequireGraph>>,
    layout: layout::Layout,
    /// The edit the graph was taken at.
    at: Option<u64>,
    /// The edit a graph is queued or running for; dropping the task cancels
    /// it.
    pending: Option<(u64, Task<()>)>,
    _observe: Subscription,
}

impl Shell {
    /// Brings the Require Graph forward, opening it if it isn't.
    pub(crate) fn open_require_graph(&mut self, cx: &mut Context<Self>) {
        if !crate::beta::enabled(BETA_ID) {
            return;
        }
        if let Some(existing) = &self.require_graph_window {
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
            let opened = RequireGraphWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.require_graph_window = opened);
        });
    }
}

impl RequireGraphWindow {
    fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(tokens::scaled_width(WIDTH), tokens::scaled_width(HEIGHT)),
                cx,
            ))),
            is_resizable: true,
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some("Require Graph".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| RequireGraphWindow::new(shell, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&shell, |this: &mut Self, _, cx| this.queue(cx));
        let mut this = RequireGraphWindow {
            shell,
            graph: None,
            layout: layout::Layout::default(),
            at: None,
            pending: None,
            _observe: observe,
        };
        this.queue(cx);
        this
    }

    /// Graphs the place in the background when it changed since the last
    /// graph, after [`QUIET`] unless there is no graph to show yet.
    fn queue(&mut self, cx: &mut Context<Self>) {
        let stamp = self.shell.read(cx).edit_dom().mutations();
        if self.at == Some(stamp) || self.pending.as_ref().is_some_and(|p| p.0 == stamp) {
            return;
        }
        let wait = self.at.is_some();
        let shell = self.shell.downgrade();
        let task = cx.spawn(async move |this, cx| {
            if wait {
                cx.background_executor().timer(QUIET).await;
            }
            // Edited again while waiting: that edit's notify queues the next.
            let Ok(Some(copy)) = shell.update(cx, |shell, _| {
                (shell.edit_dom().mutations() == stamp)
                    .then(|| rbx_audit::slim_copy(shell.edit_dom()))
            }) else {
                return;
            };
            let graph = cx
                .background_spawn(async move { rbx_audit::require_graph(&copy) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.layout = layout::Layout::of(&graph);
                this.graph = Some(Arc::new(graph));
                this.at = Some(stamp);
                this.pending = None;
                cx.notify();
            });
        });
        self.pending = Some((stamp, task));
    }

    /// Opens `script` in the editor's window, at `line` (1-based) if given.
    fn reveal(&self, script: Ref, line: Option<usize>, cx: &mut Context<Self>) {
        let shell = self.shell.clone();
        let main = shell.read(cx).window_handle;
        let _ = main.update(cx, |_, window, cx| {
            window.activate_window();
            shell.update(cx, |shell, cx| match line {
                Some(line) => {
                    let at = lsp_types::Position::new(line.saturating_sub(1) as u32, 0);
                    shell.reveal(script, lsp_types::Range::new(at, at), window, cx);
                }
                None => shell.open_place_script(script, window, cx),
            });
        });
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = match &self.graph {
            None => "Graphing\u{2026}".to_string(),
            Some(graph) => {
                let unknown = graph
                    .edges
                    .iter()
                    .filter(|e| matches!(e.target, Target::Unknown(_)))
                    .count();
                format!(
                    "{} scripts \u{b7} {} requires \u{b7} {} cycles \u{b7} {} unknown",
                    graph.nodes.len(),
                    graph.edges.len(),
                    graph.cycles.len(),
                    unknown
                )
            }
        };
        h_flex()
            .w_full()
            .flex_none()
            .gap(px(16.))
            .px(px(16.))
            .py(px(8.))
            .items_center()
            .border_b_1()
            .border_color(tokens::border())
            .child(
                div()
                    .text_color(tokens::text2())
                    .font_family(tokens::font_mono())
                    .child(summary),
            )
            .child(view::legend())
            .child(div().flex_1())
            .child(
                ui::button("require-graph-rerun", "Re-run", ui::Weight::Secondary, true).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.at = None;
                        this.pending = None;
                        this.queue(cx);
                        cx.notify();
                    }),
                ),
            )
    }
}

impl Render for RequireGraphWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("require-graph-window")
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text())
            .child(super::chrome::window_topbar(
                "Require Graph".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(self.toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.canvas(cx))
                    .child(self.issues(cx)),
            )
    }
}
