//! The Argon review prompt's "Diff" window: the pending batch as a list
//! of changes on the left (additions with their subtrees, updates,
//! removals) and the selected change on the right — its properties before
//! and after, a script's source as a unified diff, what an added or
//! removed container holds — with Accept and Cancel in its footer,
//! calling the dock's own handlers. Resizable; under 760 px of content
//! the list gives way to a picker bar over the detail.
//!
//! **This window holds no copy of the batch.** It reads
//! [`Shell::argon_diff_nodes`] off whatever review is pending, keyed on
//! the review's serial, so Accept or Cancel from the dock is reflected
//! here at once; what it caches — the tree, and a script's line diff —
//! is dropped when the serial changes. Once nothing is pending (the review
//! resolved, or the debug variable that can seed one, see
//! [`super::argon_sync::DIFF_VARIABLE`], never ran) there is nothing left
//! to show, so the window closes itself rather than sit there empty.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Root};
use gpui_kit::*;

use crate::tokens;

use super::argon_sync::{ChangeKind, DiffNode};
use super::Shell;

mod code;
mod detail;
mod diff;
mod header;
mod list;
mod model;
mod state;
mod values;

use code::{CodeRow, Source};
use model::Filter;

const CONTENT_WIDTH: f32 = 1100.0;
const CONTENT_HEIGHT: f32 = 720.0;
const MIN_WIDTH: f32 = 520.0;
const MIN_HEIGHT: f32 = 440.0;
/// Content width from which the list pane sits beside the detail.
const WIDE_MIN: f32 = 760.0;

/// `RBX_STUDIO_ARGON_DIFF_SELECT=<name>`, `RBX_STUDIO_ARGON_DIFF_COLLAPSE=
/// additions,updates,removals` and `RBX_STUDIO_ARGON_DIFF_EXPAND=<name>,
/// <name>`: the window's opening state for a capture, read once when it
/// opens. See `argon_sync::diff_fixture`.
const SELECT_VARIABLE: &str = "RBX_STUDIO_ARGON_DIFF_SELECT";
const COLLAPSE_VARIABLE: &str = "RBX_STUDIO_ARGON_DIFF_COLLAPSE";
const EXPAND_VARIABLE: &str = "RBX_STUDIO_ARGON_DIFF_EXPAND";

/// The card's rows for one `(serial, node)`.
type CodeRows = Option<((u64, usize), Rc<Vec<CodeRow>>)>;

/// A script's line diff, computed once per batch and node.
struct CachedDiff {
    old: Source,
    new: Source,
    diff: diff::LineDiff,
}

pub(crate) struct ArgonDiffWindow {
    shell: Entity<Shell>,
    grab: Rc<Cell<bool>>,
    filter: Filter,
    search: Entity<InputState>,
    _search_changed: Subscription,
    selected: Option<usize>,
    expanded: HashSet<usize>,
    collapsed: HashSet<ChangeKind>,
    picker_open: bool,
    list_scroll: ScrollHandle,
    /// The tree of the review with this serial.
    nodes: Option<(u64, Rc<Vec<DiffNode>>)>,
    diffs: HashMap<(u64, usize), Rc<CachedDiff>>,
    /// The card's rows for `(serial, node)`, with the hunks opened so far.
    code_rows: CodeRows,
    code_list: ListState,
    expanded_hunks: HashSet<usize>,
    width: Rc<Cell<f32>>,
    /// Applied on the first frame, once the tree is known.
    initial: Option<(Option<String>, Vec<String>, Vec<String>)>,
}

impl ArgonDiffWindow {
    pub(super) fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let project = shell.read(cx).argon_project_name();
        // The window's content is the whole of these: the title bar is the
        // editor's own, drawn inside.
        let window_size = size(
            tokens::scaled_width(CONTENT_WIDTH),
            tokens::scaled_width(CONTENT_HEIGHT),
        );
        let min_size = size(
            tokens::scaled_width(MIN_WIDTH),
            tokens::scaled_width(MIN_HEIGHT),
        );
        let title = SharedString::from(format!("Argon Diff \u{2014} {project}"));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                window_size,
                cx,
            ))),
            kind: WindowKind::Floating,
            is_resizable: true,
            is_minimizable: false,
            window_min_size: Some(min_size),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some(title.clone()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };

        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| {
                let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter changes"));
                let search_changed =
                    cx.subscribe(&search, |this: &mut Self, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.ensure_selection(cx);
                            cx.notify();
                        }
                    });
                let list = |name: &str| -> Vec<String> {
                    std::env::var(name)
                        .map(|value| {
                            value
                                .split(',')
                                .map(|s| s.trim().to_owned())
                                .filter(|s| !s.is_empty())
                                .collect()
                        })
                        .unwrap_or_default()
                };
                ArgonDiffWindow {
                    shell,
                    grab: Rc::new(Cell::new(false)),
                    filter: Filter::All,
                    search,
                    _search_changed: search_changed,
                    selected: None,
                    expanded: HashSet::new(),
                    collapsed: HashSet::new(),
                    picker_open: false,
                    list_scroll: ScrollHandle::new(),
                    nodes: None,
                    diffs: HashMap::new(),
                    code_rows: None,
                    code_list: ListState::new(0, ListAlignment::Top, px(200.)),
                    expanded_hunks: HashSet::new(),
                    width: Rc::new(Cell::new(CONTENT_WIDTH)),
                    initial: Some((
                        std::env::var(SELECT_VARIABLE).ok(),
                        list(COLLAPSE_VARIABLE),
                        list(EXPAND_VARIABLE),
                    )),
                }
            });
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }
}

impl Render for ArgonDiffWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let nodes = self.nodes(cx);
        if nodes.is_empty() {
            // Nothing pending any more — a window showing an empty review
            // is worse than no window.
            window.remove_window();
            return div().into_any_element();
        }
        self.apply_initial(&nodes, cx);
        // Every script's diff is wanted up front: the rows quote their
        // counts before any of them is selected.
        for (node, _) in model::walk(&nodes) {
            if node
                .source
                .as_ref()
                .is_some_and(|s| s.old.is_some() && s.new.is_some())
            {
                self.cached_diff(node, cx);
            }
        }
        let rows = self.rows(&nodes, cx);
        if self.selected.is_none() {
            self.ensure_selection(cx);
        }
        let selected = self
            .selected
            .and_then(|id| model::find(&nodes, id).cloned());
        let narrow = self.width.get() < WIDE_MIN;
        let counts = model::counts(&nodes);
        let (project, pack) = {
            let shell = self.shell.read(cx);
            (shell.argon_project_name(), shell.icon_pack())
        };
        let title = SharedString::from(format!("Argon Diff \u{2014} {project}"));

        let header = self.header(&project, counts, narrow, cx);
        let footer = self.footer(narrow, cx);
        let properties = selected
            .as_ref()
            .map(|node| values::format_properties(self.shell.read(cx), node))
            .unwrap_or_default();
        let detail = self.detail_pane(selected.as_ref(), &properties, pack, narrow, cx);

        let body: AnyElement = if narrow {
            let picker = self.picker_bar(&nodes, &rows, pack, cx);
            let menu = self.picker_open.then(|| {
                div()
                    .absolute()
                    .top(px(45.))
                    .left(px(14.))
                    .right(px(14.))
                    .max_h(px(360.))
                    .rounded(tokens::RADIUS)
                    .bg(tokens::dock())
                    .border_1()
                    .border_color(tokens::border2())
                    .shadow(vec![tokens::floating_shadow()])
                    .child(
                        self.list_pane(&nodes, &rows, pack, cx)
                            .w_full()
                            .border_r_0(),
                    )
            });
            v_flex()
                .flex_1()
                .min_h_0()
                .relative()
                .child(picker)
                .child(detail)
                .children(menu)
                .into_any_element()
        } else {
            h_flex()
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(self.list_pane(&nodes, &rows, pack, cx))
                .child(detail)
                .into_any_element()
        };

        let width = self.width.clone();
        let this = cx.entity_id();
        v_flex()
            .id("argon-diff-window")
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_color(tokens::text())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.on_key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(super::chrome::panel_topbar(
                title,
                self.grab.clone(),
                |_, window: &mut Window, _| window.remove_window(),
            ))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .on_children_prepainted(move |bounds, window, cx| {
                        if let Some(bounds) = bounds.first() {
                            let w = f32::from(bounds.size.width);
                            if (w - width.get()).abs() >= 0.5 {
                                width.set(w);
                                cx.notify(this);
                                let handle = window.window_handle();
                                cx.defer(move |cx| {
                                    let _ = handle.update(cx, |_, window, _| window.refresh());
                                });
                            }
                        }
                    })
                    .child(body),
            )
            .child(footer)
            .into_any_element()
    }
}
