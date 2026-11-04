//! File › Version History…: the linked place's saved and published
//! versions, newest first, a page at a time, with who saved each one.
//! Two things can be done with a version, as in Studio and the Creator
//! Dashboard (`creator-docs`, `projects/version-history.md`):
//!
//! - **Open** downloads it as an unlinked local copy and opens that in a new
//!   editor, Studio's "Open Local Copy".
//! - **Restore** uploads it as a new version of the place, Open Cloud has
//!   no revert call, and Roblox's own restore is exactly that. Roblox's
//!   restore only saves; publishing it too is offered beside it. Both are
//!   asked first, like every upload (see the parent module).
//!
//! The window keeps no copy of the link: it reads it from `links.json` when
//! it opens and again whenever [`RobloxPublish::changes`] moves, so linking
//! from here (through the same game picker) or publishing from the File
//! menu shows up without reopening it.
//!
//! `RBX_STUDIO_ROBLOX=history` opens it with the editor; see `fetch` for
//! the capture mock.
//!
//! [`RobloxPublish::changes`]: super::RobloxPublish::changes

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::Root;
use gpui_kit::*;
use rbx_cloud::PublishMode;

use crate::command_bar::Feedback;
use crate::home::{self, Link};
use crate::tokens;

use super::{Failure, Shell, Target, SOURCE};

mod dialogs;
mod fetch;
mod labels;
mod model;
mod view;

const WIDTH: f32 = 780.;
const HEIGHT: f32 = 640.;
const MIN_WIDTH: f32 = 560.;
const MIN_HEIGHT: f32 = 420.;

#[derive(Debug, PartialEq, Eq)]
enum Listing {
    Loading,
    Ready,
    Failed(String),
}

#[derive(Debug, PartialEq, Eq)]
enum Dialog {
    Restore {
        version: u64,
    },
    Failed {
        version: u64,
        mode: PublishMode,
        failure: Failure,
    },
}

/// The line above the list saying how the last Open or Restore went.
struct Notice {
    ok: bool,
    text: String,
}

pub(crate) struct HistoryWindow {
    shell: Entity<Shell>,
    link: Option<Link>,
    history: model::History,
    listing: Listing,
    loading_more: bool,
    dialog: Option<Dialog>,
    /// What is running, an Open or a Restore. One at a time: a second
    /// Restore while the first uploads would make two versions.
    busy: Option<String>,
    notice: Option<Notice>,
    /// The shell's `roblox.changes` this window last caught up with.
    seen: u64,
    /// Narrows the listed versions, see [`model::History::matches`].
    search: Entity<InputState>,
    /// The date filter beside it.
    since: model::Since,
    /// The linked place's local labels, see `labels`.
    labels: labels::Labels,
    /// The version whose label is being typed into `label_input`.
    editing: Option<u64>,
    label_input: Entity<InputState>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// Brings Version History forward, opening it if it isn't.
    pub(crate) fn open_version_history(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.version_history {
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
            let opened = HistoryWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.version_history = opened);
        });
    }
}

impl HistoryWindow {
    fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            app_id: Some(crate::APP_ID.to_owned()),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(tokens::scaled_width(WIDTH), tokens::scaled_width(HEIGHT)),
                cx,
            ))),
            is_resizable: true,
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some("Version History".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| HistoryWindow::new(shell, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search by date, who saved it, or label")
                .clean_on_escape()
        });
        let label_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Label this version"));
        let subscriptions = vec![
            cx.observe(&shell, |this, shell, cx| {
                let changes = shell.read(cx).roblox.changes;
                if changes != this.seen {
                    this.seen = changes;
                    this.relink(cx);
                }
            }),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(
                &label_input,
                |this, _, event: &InputEvent, cx| match event {
                    InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_label(cx),
                    _ => {}
                },
            ),
        ];
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let seen = shell.read(cx).roblox.changes;
        let link = home::link_of(&shell.read(cx).path);
        let mut this = HistoryWindow {
            labels: link
                .as_ref()
                .map(|link| labels::load(link.place_id))
                .unwrap_or_default(),
            link,
            shell,
            history: model::History::default(),
            listing: Listing::Loading,
            loading_more: false,
            dialog: None,
            busy: None,
            notice: None,
            seen,
            search,
            since: model::Since::default(),
            editing: None,
            label_input,
            focus,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        this.reload(None, cx);
        this
    }

    fn target(&self) -> Option<Target> {
        self.link.as_ref().map(|link| Target {
            universe_id: link.universe_id,
            place_id: link.place_id,
        })
    }

    /// Re-reads the file's link and starts over from the first page.
    fn relink(&mut self, cx: &mut Context<Self>) {
        self.link = home::link_of(&self.shell.read(cx).path);
        self.labels = self
            .link
            .as_ref()
            .map(|link| labels::load(link.place_id))
            .unwrap_or_default();
        self.editing = None;
        self.reload(None, cx);
    }

    /// The first page again, filtered to `filter`'s versions.
    fn reload(&mut self, filter: Option<u64>, cx: &mut Context<Self>) {
        let generation = self.history.reset(filter);
        self.loading_more = false;
        self.listing = Listing::Loading;
        self.fetch(generation, None, cx);
        cx.notify();
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(cursor) = self.history.next_cursor.clone() else {
            return;
        };
        if self.loading_more {
            return;
        }
        self.loading_more = true;
        self.fetch(self.history.generation(), Some(cursor), cx);
        cx.notify();
    }

    fn fetch(&mut self, generation: u64, cursor: Option<String>, cx: &mut Context<Self>) {
        let Some(target) = self.target() else {
            self.listing = Listing::Ready;
            return;
        };
        let filter = self.history.filter;
        let known = self.history.known_names();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(
                    async move { fetch::load(target.place_id, cursor, filter, known) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.history.generation() {
                    return;
                }
                this.loading_more = false;
                match result {
                    Ok(loaded) => {
                        this.history.learn_names(loaded.names);
                        if let Some(contributors) = loaded.contributors {
                            this.history.contributors = contributors;
                        }
                        this.history.accept(generation, loaded.page);
                        this.listing = Listing::Ready;
                    }
                    // A failed "Load more" keeps what is listed and says so
                    // where the button was.
                    Err(message) if !this.history.versions.is_empty() => {
                        this.notice = Some(Notice {
                            ok: false,
                            text: message,
                        })
                    }
                    Err(message) => this.listing = Listing::Failed(message),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Studio's Clear filters: the search, the date range and the person.
    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.since = model::Since::AllTime;
        if self.history.filter.is_some() {
            self.reload(None, cx);
        }
        cx.notify();
    }

    /// Opens `version`'s label for typing, holding the label it has.
    fn edit_label(&mut self, version: u64, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.labels.get(&version).cloned().unwrap_or_default();
        self.editing = Some(version);
        self.label_input.update(cx, |input, cx| {
            input.set_value(current, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// Saves the label being typed (Enter, or clicking away); a blank one
    /// takes the label off.
    fn commit_label(&mut self, cx: &mut Context<Self>) {
        let (Some(version), Some(link)) = (self.editing.take(), self.link.as_ref()) else {
            return;
        };
        let text = self.label_input.read(cx).value().to_string();
        if self.labels.get(&version).map(String::as_str).unwrap_or("") != text.trim() {
            match labels::save(link.place_id, version, &text) {
                Ok(labels) => self.labels = labels,
                Err(message) => {
                    self.notice = Some(Notice {
                        ok: false,
                        text: format!("The label couldn\u{2019}t be saved: {message}"),
                    })
                }
            }
        }
        cx.notify();
    }

    fn link_place(&mut self, cx: &mut Context<Self>) {
        self.shell
            .update(cx, |shell, cx| shell.open_roblox_link(None, cx));
    }

    fn open_version(&mut self, version: u64, cx: &mut Context<Self>) {
        let Some(target) = self.target().filter(|_| self.busy.is_none()) else {
            return;
        };
        self.start(format!("Downloading version {version}\u{2026}"), cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { fetch::open_copy(target.place_id, version) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let line = match result {
                    Ok(path) => Ok(format!(
                        "Opened version {version} of place {} as a local copy: {}",
                        target.place_id,
                        path.display()
                    )),
                    Err(message) => Err(format!(
                        "Opening version {version} of place {} failed: {message}",
                        target.place_id
                    )),
                };
                this.finish(line, None, cx);
            });
        })
        .detach();
    }

    fn ask_restore(&mut self, version: u64, cx: &mut Context<Self>) {
        if self.busy.is_none() {
            self.dialog = Some(Dialog::Restore { version });
            cx.notify();
        }
    }

    /// A confirmation's button, Enter in it, or a failure's Try again.
    fn restore(&mut self, version: u64, mode: PublishMode, cx: &mut Context<Self>) {
        self.dialog = None;
        let Some(target) = self.target().filter(|_| self.busy.is_none()) else {
            return;
        };
        self.start(format!("Restoring version {version}\u{2026}"), cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { fetch::restore(target, version, mode) })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(restored) => {
                    let line = restored_line(target.place_id, version, restored.version, mode);
                    this.finish(Ok(line), restored.warning, cx);
                    this.reload(this.history.filter, cx);
                }
                Err(failure) => {
                    let line = format!(
                        "Restoring version {version} of place {} failed: {}",
                        target.place_id, failure.message
                    );
                    this.finish(Err(line), None, cx);
                    this.dialog = Some(Dialog::Failed {
                        version,
                        mode,
                        failure,
                    });
                }
            });
        })
        .detach();
    }

    fn start(&mut self, what: String, cx: &mut Context<Self>) {
        self.notice = None;
        self.busy = Some(what);
        cx.notify();
    }

    /// Ends an Open or Restore: the window's notice, and the same line in
    /// the Output dock and the Command Bar as an upload's.
    fn finish(
        &mut self,
        line: Result<String, String>,
        warning: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.busy = None;
        let (ok, text) = match line {
            Ok(text) => (true, text),
            Err(text) => (false, text),
        };
        let feedback = if ok {
            Feedback::Output(text.clone())
        } else {
            Feedback::Error(text.clone())
        };
        self.notice = Some(Notice { ok, text });
        self.shell.update(cx, |shell, cx| {
            shell.output.push(SOURCE, feedback.clone());
            shell.command_bar.set_feedback(feedback);
            if let Some(warning) = warning {
                shell.output.push(SOURCE, Feedback::Warning(warning));
            }
            cx.notify();
        });
        cx.notify();
    }

    fn handle_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        match (keystroke.key.as_str(), &self.dialog) {
            ("escape", Some(_)) => self.dialog = None,
            ("escape", None) if self.editing.is_some() => self.editing = None,
            ("enter", Some(Dialog::Restore { version })) if !keystroke.modifiers.modified() => {
                let version = *version;
                self.restore(version, PublishMode::Saved, cx);
            }
            _ => return false,
        }
        true
    }
}

/// What a successful restore says, naming the version it made and whether
/// players now get it.
fn restored_line(place_id: u64, from: u64, to: u64, mode: PublishMode) -> String {
    let published = match mode {
        PublishMode::Saved => "saved, not published",
        PublishMode::Published => "published",
    };
    format!("Restored version {from} of place {place_id} as version {to} ({published})")
}

#[cfg(test)]
mod tests;
