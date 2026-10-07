//! The Figma window's state and what its buttons do; `view` draws it.
//!
//! Three pages: Home (sign-in, a link field, recent files), a file's node
//! tree (loaded a level at a time, one preview of the selected row), and
//! Review (the inferred tree, with each row's class and a flatten toggle).
//! Nothing touches the place until Import.
//!
//! Captures: `RBX_STUDIO_FIGMA=home|file|review` opens the window at
//! startup on that page, `file` and `review` on the first recent file
//! (`review` on its node `1:2`). `RBX_STUDIO_FIGMA_EXPAND=id,id` expands
//! rows in order and `RBX_STUDIO_FIGMA_SELECT=id` selects one;
//! `RBX_STUDIO_FIGMA_MENU=id` opens a review row's class menu.
//!
//! Parity checks: `RBX_STUDIO_FIGMA_IMPORT=<frame link>` imports that frame
//! the way Import does (on a fresh full-screen `ScreenGui`, so pair it with
//! `RBX_STUDIO_UI_EDITOR=1`), then sizes the canvas to the frame; with a
//! file link it opens the file instead. `RBX_STUDIO_FIGMA_REFERENCE=<dir>`
//! saves Figma's own 1x render of that frame there as `<id>.figma.png`, and
//! `RBX_STUDIO_FIGMA_OUTLINE=<path>` writes each opened file's top-level
//! nodes (page, id, type, name) to `path`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::Root;
use gpui_kit::*;
use rbx_figma::browse::{Outline, Recent};
use rbx_figma::infer::Node;
use rbx_figma::link::{self, Link};
use rbx_figma::oauth;

use super::model::Edits;
use super::source::{self, Reply, Source};
use super::Shell;
use crate::{figma_store, tokens};

pub(in crate::shell) const OPEN_VARIABLE: &str = "RBX_STUDIO_FIGMA";
const EXPAND_VARIABLE: &str = "RBX_STUDIO_FIGMA_EXPAND";
const SELECT_VARIABLE: &str = "RBX_STUDIO_FIGMA_SELECT";
const MENU_VARIABLE: &str = "RBX_STUDIO_FIGMA_MENU";
pub(in crate::shell) const IMPORT_VARIABLE: &str = "RBX_STUDIO_FIGMA_IMPORT";
const REFERENCE_VARIABLE: &str = "RBX_STUDIO_FIGMA_REFERENCE";
const OUTLINE_VARIABLE: &str = "RBX_STUDIO_FIGMA_OUTLINE";

const WIDTH: f32 = 960.;
const HEIGHT: f32 = 640.;
/// How long the browser has to come back from Figma's consent page.
const SIGN_IN_WAIT: Duration = Duration::from_secs(180);

#[allow(clippy::large_enum_variant)] // One per window; boxing saves nothing.
pub(super) enum Page {
    Home,
    File(Browse),
    Review(Review),
}

/// One file's tree. `outline` is `None` until it has loaded.
pub(super) struct Browse {
    pub key: String,
    pub outline: Option<Outline>,
    pub expanded: HashSet<String>,
    pub selected: Option<String>,
    pub preview: Option<PathBuf>,
    /// Bumped per selection, so a slow preview can't replace a newer one.
    preview_turn: u64,
}

pub(super) struct Review {
    pub link: Link,
    /// The inferred tree, untouched; `None` while it is being read.
    pub tree: Option<Node>,
    pub edits: Edits,
    /// The row whose class menu is open.
    pub menu: Option<String>,
    /// The file view to go back to, when review came from one.
    from: Option<Browse>,
}

pub(super) struct FigmaWindow {
    shell: Entity<Shell>,
    /// `None` while signed out.
    pub source: Option<Source>,
    pub link: Entity<InputState>,
    pub filter: Entity<InputState>,
    pub page: Page,
    pub recent: Recent,
    pub thumbs: HashMap<String, PathBuf>,
    /// Background calls in flight.
    pub busy: usize,
    pub progress: Arc<Mutex<String>>,
    /// The last outcome, `true` for an error.
    pub status: Option<(bool, String)>,
    pub focus: FocusHandle,
    /// Capture variables still to act on; see the module doc.
    expand_queue: VecDeque<String>,
    select_after: Option<String>,
    menu_after: Option<String>,
    debug_page: Option<String>,
    /// `RBX_STUDIO_FIGMA_IMPORT`: import as soon as review has the tree.
    auto_import: bool,
    _subscriptions: Vec<Subscription>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn split_ids(var: &str) -> VecDeque<String> {
    std::env::var(var)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

impl FigmaWindow {
    pub(super) fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(tokens::scaled_width(WIDTH), tokens::scaled_width(HEIGHT)),
                cx,
            ))),
            is_resizable: true,
            window_min_size: Some(size(px(640.), px(420.))),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some("Import from Figma".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| FigmaWindow::new(shell, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let link = cx
            .new(|cx| InputState::new(window, cx).placeholder("Paste a Figma file or frame link"));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter loaded layers"));
        let subscriptions = vec![
            cx.subscribe(&link, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.open_link(cx);
                }
            }),
            cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let source = Source::fixture();
        let mut this = FigmaWindow {
            shell,
            recent: source::recent(source.as_ref()),
            source,
            link,
            filter,
            page: Page::Home,
            thumbs: HashMap::new(),
            busy: 0,
            progress: Arc::default(),
            status: None,
            focus,
            expand_queue: split_ids(EXPAND_VARIABLE),
            select_after: std::env::var(SELECT_VARIABLE).ok(),
            menu_after: std::env::var(MENU_VARIABLE).ok(),
            debug_page: std::env::var(OPEN_VARIABLE).ok(),
            auto_import: false,
            _subscriptions: subscriptions,
        };
        if this.source.is_some() {
            this.connected(cx);
        } else {
            cx.spawn(async move |this, cx| {
                let stored = figma_store::load(cx).await.ok().flatten();
                let _ = this.update(cx, |this, cx| {
                    if this.source.is_none() {
                        if let Some(tokens) = stored {
                            this.source = Some(Source::Live(tokens));
                            this.connected(cx);
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        this
    }

    /// Signed in (or on a fixture): fetch the thumbnails, then the page a
    /// capture asked for.
    fn connected(&mut self, cx: &mut Context<Self>) {
        let files = self.recent.files.clone();
        self.call(
            move |source| source.thumbnails(files),
            |this, result, _| {
                if let Ok(thumbs) = result {
                    this.thumbs.extend(thumbs);
                }
            },
            cx,
        );
        let first = self.recent.files.first().map(|f| f.key.clone());
        match (self.debug_page.take().as_deref(), first) {
            (Some("file"), Some(key)) => self.open_file(key, cx),
            (Some("review"), Some(key)) => self.review(
                Link {
                    file_key: key,
                    node_id: "1:2".into(),
                },
                cx,
            ),
            _ => {}
        }
        if let Ok(text) = std::env::var(IMPORT_VARIABLE) {
            match link::parse_any(&text) {
                Ok((key, None)) => self.open_file(key, cx),
                Ok((file_key, Some(node_id))) => {
                    let link = Link { file_key, node_id };
                    if let Some(dir) = std::env::var_os(REFERENCE_VARIABLE) {
                        let asked = link.clone();
                        self.call(
                            move |source| source.reference(asked, dir.into()),
                            |_, result, _| {
                                if let Ok(path) = result {
                                    eprintln!("rbxstudio: Figma reference at {}", path.display());
                                }
                            },
                            cx,
                        );
                    }
                    self.auto_import = true;
                    self.review(link, cx);
                }
                Err(err) => self.report(Err(err), cx),
            }
        }
    }

    pub(super) fn connect(&mut self, cx: &mut Context<Self>) {
        let pending = match oauth::begin() {
            Ok(pending) => pending,
            Err(err) => return self.report(Err(err), cx),
        };
        cx.open_url(&pending.url);
        self.start("Waiting for Figma in your browser\u{2026}", cx);
        cx.spawn(async move |this, cx| {
            let signed_in = cx
                .background_spawn(async move { pending.finish(SIGN_IN_WAIT) })
                .await;
            if let Ok(tokens) = &signed_in {
                if let Err(err) = figma_store::save(tokens, cx).await {
                    eprintln!("Figma sign-in not stored: {err}");
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.busy -= 1;
                match signed_in {
                    Ok(tokens) => {
                        this.source = Some(Source::Live(tokens));
                        this.report(Ok("Connected to Figma.".into()), cx);
                        this.connected(cx);
                    }
                    Err(err) => this.report(Err(err), cx),
                }
            });
        })
        .detach();
    }

    pub(super) fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.source = None;
        self.page = Page::Home;
        cx.spawn(async move |_, cx| {
            if let Err(err) = figma_store::forget(cx).await {
                eprintln!("Figma sign-in not removed: {err}");
            }
        })
        .detach();
        self.report(Ok("Disconnected from Figma.".into()), cx);
    }

    fn report(&mut self, outcome: Result<String, String>, cx: &mut Context<Self>) {
        self.status = Some(match outcome {
            Ok(text) => (false, text),
            Err(text) => (true, text),
        });
        cx.notify();
    }

    /// Counts a call in and keeps redrawing while any is running, so the
    /// progress line moves.
    fn start(&mut self, first: &str, cx: &mut Context<Self>) {
        *self.progress.lock().unwrap() = first.to_string();
        self.status = None;
        self.busy += 1;
        if self.busy > 1 {
            return;
        }
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let busy = this.update(cx, |this, cx| {
                cx.notify();
                this.busy
            });
            if !matches!(busy, Ok(n) if n > 0) {
                break;
            }
        })
        .detach();
    }

    /// Runs `work` on a background thread, keeps any renewed tokens (in
    /// the keyring too, whatever the result), then hands the result to
    /// `done`.
    fn call<T: Send + 'static>(
        &mut self,
        work: impl FnOnce(Source) -> Reply<T> + Send + 'static,
        done: impl FnOnce(&mut Self, Result<T, String>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.source.clone() else {
            return self.report(Err("Connect to Figma first.".into()), cx);
        };
        self.start("Asking Figma\u{2026}", cx);
        cx.spawn(async move |this, cx| {
            let (result, refreshed) = cx.background_spawn(async move { work(source) }).await;
            if let Some(tokens) = &refreshed {
                if let Err(err) = figma_store::save(tokens, cx).await {
                    eprintln!("Refreshed Figma sign-in not stored: {err}");
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.busy = this.busy.saturating_sub(1);
                if let (Some(tokens), Some(Source::Live(_))) = (refreshed, &this.source) {
                    this.source = Some(Source::Live(tokens));
                }
                if let Err(err) = &result {
                    this.report(Err(err.clone()), cx);
                }
                done(this, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The link field: a file opens its tree, a frame goes straight to review.
    pub(super) fn open_link(&mut self, cx: &mut Context<Self>) {
        let text = self.link.read(cx).value().to_string();
        match link::parse_any(&text) {
            Ok((key, None)) => self.open_file(key, cx),
            Ok((file_key, Some(node_id))) => self.review(Link { file_key, node_id }, cx),
            Err(err) => self.report(Err(err), cx),
        }
    }

    pub(super) fn home(&mut self, cx: &mut Context<Self>) {
        self.page = Page::Home;
        cx.notify();
    }

    pub(super) fn open_file(&mut self, key: String, cx: &mut Context<Self>) {
        self.page = Page::File(Browse {
            key: key.clone(),
            outline: None,
            expanded: HashSet::new(),
            selected: None,
            preview: None,
            preview_turn: 0,
        });
        let asked = key.clone();
        self.call(
            move |source| source.outline(asked),
            move |this, result, cx| {
                let Ok(outline) = result else { return };
                let Page::File(browse) = &mut this.page else {
                    return;
                };
                if browse.key != key {
                    return;
                }
                this.recent.touch(outline.recent(now()));
                if let Some(url) = &outline.thumbnail_url {
                    if !this.thumbs.contains_key(&key) && url.starts_with("http") {
                        let files = vec![outline.recent(now())];
                        this.call(
                            move |source| source.thumbnails(files),
                            |this, result, _| {
                                if let Ok(thumbs) = result {
                                    this.thumbs.extend(thumbs);
                                }
                            },
                            cx,
                        );
                    }
                }
                if let Some(path) = std::env::var_os(OUTLINE_VARIABLE) {
                    let mut lines = String::new();
                    for page in &outline.pages {
                        for item in page.children.iter().flatten() {
                            lines += &format!(
                                "{}\t{}\t{}\t{}\n",
                                page.name, item.id, item.kind, item.name
                            );
                        }
                    }
                    let _ = std::fs::write(path, lines);
                }
                if let Page::File(browse) = &mut this.page {
                    browse.outline = Some(outline);
                }
                this.next_expansion(cx);
            },
            cx,
        );
    }

    /// Opens or closes a row, loading its children the first time.
    pub(super) fn toggle(&mut self, id: String, cx: &mut Context<Self>) {
        let Page::File(browse) = &mut self.page else {
            return;
        };
        if browse.expanded.remove(&id) {
            return cx.notify();
        }
        browse.expanded.insert(id.clone());
        let loaded = browse
            .outline
            .as_mut()
            .and_then(|o| o.pages.iter_mut().find_map(|p| p.find_mut(&id)))
            .is_some_and(|item| item.children.is_some());
        if loaded {
            cx.notify();
            return self.next_expansion(cx);
        }
        let key = browse.key.clone();
        let asked = id.clone();
        self.call(
            move |source| source.children(key, asked),
            move |this, result, cx| {
                let Page::File(browse) = &mut this.page else {
                    return;
                };
                let item = browse
                    .outline
                    .as_mut()
                    .and_then(|o| o.pages.iter_mut().find_map(|p| p.find_mut(&id)));
                match (item, result) {
                    (Some(item), Ok(children)) => item.children = Some(children),
                    _ => {
                        browse.expanded.remove(&id);
                    }
                }
                this.next_expansion(cx);
            },
            cx,
        );
    }

    /// The capture's next queued expansion, then its selection.
    fn next_expansion(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.expand_queue.pop_front() {
            return self.toggle(id, cx);
        }
        if let Some(id) = self.select_after.take() {
            self.select(id, cx);
        }
    }

    pub(super) fn select(&mut self, id: String, cx: &mut Context<Self>) {
        let Page::File(browse) = &mut self.page else {
            return;
        };
        if browse.selected.as_deref() == Some(id.as_str()) {
            return;
        }
        browse.selected = Some(id.clone());
        browse.preview = None;
        browse.preview_turn += 1;
        let turn = browse.preview_turn;
        let key = browse.key.clone();
        self.call(
            move |source| source.preview(key, id),
            move |this, result, _| {
                if let (Page::File(browse), Ok(path)) = (&mut this.page, result) {
                    if browse.preview_turn == turn {
                        browse.preview = Some(path);
                    }
                }
            },
            cx,
        );
    }

    /// Review of the selected row, coming back to this file on Back.
    pub(super) fn review_selected(&mut self, cx: &mut Context<Self>) {
        let Page::File(browse) = &self.page else {
            return;
        };
        let Some(node_id) = browse.selected.clone() else {
            return;
        };
        let file_key = browse.key.clone();
        self.review(Link { file_key, node_id }, cx);
    }

    fn review(&mut self, link: Link, cx: &mut Context<Self>) {
        let from = match std::mem::replace(&mut self.page, Page::Home) {
            Page::File(browse) => Some(browse),
            _ => None,
        };
        self.page = Page::Review(Review {
            link: link.clone(),
            tree: None,
            edits: Edits::default(),
            menu: None,
            from,
        });
        let progress = self.progress.clone();
        self.call(
            move |source| source.prepare(link, progress),
            |this, result, cx| {
                let menu = this.menu_after.take();
                if let (Page::Review(review), Ok(tree)) = (&mut this.page, result) {
                    review.tree = Some(tree);
                    review.menu = menu;
                    if this.auto_import {
                        this.import(cx);
                    }
                }
            },
            cx,
        );
    }

    pub(super) fn back(&mut self, cx: &mut Context<Self>) {
        self.page = match std::mem::replace(&mut self.page, Page::Home) {
            Page::Review(Review {
                from: Some(browse), ..
            }) => Page::File(browse),
            _ => Page::Home,
        };
        self.status = None;
        cx.notify();
    }

    /// Uploads the reviewed tree's pictures, then inserts it in the place.
    pub(super) fn import(&mut self, cx: &mut Context<Self>) {
        let Page::Review(review) = &self.page else {
            return;
        };
        let Some(tree) = &review.tree else { return };
        let tree = super::model::apply(tree.clone(), &review.edits);
        let file_key = review.link.file_key.clone();
        let progress = self.progress.clone();
        self.call(
            move |source| source.finish(file_key, tree, progress),
            |this, result, cx| {
                let Ok(tree) = result else { return };
                let auto = std::mem::take(&mut this.auto_import);
                let outcome = this.shell.update(cx, |shell, cx| {
                    let size = auto.then(|| super::frame_size(&tree)).flatten();
                    let outcome = shell.insert_figma_tree(tree, cx);
                    shell.figma_feedback(&outcome, cx);
                    if let Some(size) = size {
                        shell.set_resolution(size, cx);
                    }
                    outcome
                });
                this.report(outcome, cx);
            },
            cx,
        );
    }

    pub(super) fn pick_class(&mut self, id: String, class: &'static str, cx: &mut Context<Self>) {
        if let Page::Review(review) = &mut self.page {
            if let Some(inferred) = review
                .tree
                .as_ref()
                .and_then(|t| super::model::find(t, &id))
            {
                review.edits.set_class(&id, class, inferred.class);
            }
            review.menu = None;
        }
        cx.notify();
    }

    pub(super) fn toggle_menu(&mut self, id: String, cx: &mut Context<Self>) {
        if let Page::Review(review) = &mut self.page {
            review.menu = (review.menu.as_deref() != Some(id.as_str())).then_some(id);
        }
        cx.notify();
    }

    pub(super) fn toggle_flat(&mut self, id: String, cx: &mut Context<Self>) {
        if let Page::Review(review) = &mut self.page {
            review.edits.toggle_flat(&id);
        }
        cx.notify();
    }
}

impl Focusable for FigmaWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
