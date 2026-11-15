//! Avatar › Upload Item: checks the item, then uploads it as a `Model`
//! through Open Cloud and hands over to the Creator Hub.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::Root;
use gpui_kit::*;

use super::super::Shell;
use super::check::Check;
use super::upload::{self, Creator, Payload};
use super::{ItemKind, ItemSource};
use crate::tokens;

mod fees;
mod view;

const WIDTH: f32 = 840.;
const HEIGHT: f32 = 760.;
const MIN_WIDTH: f32 = 680.;
const MIN_HEIGHT: f32 = 560.;

enum Phase {
    Idle,
    Sending,
    Failed(String),
    Done(u64),
}

struct UploadWindow {
    shell: Entity<Shell>,
    check: Check,
    scroll: ScrollHandle,
    title: Entity<InputState>,
    description: Entity<InputState>,
    creator: Creator,
    groups: Vec<rbx_cloud::Group>,
    phase: Phase,
    _subscriptions: Vec<Subscription>,
}

fn slot(view: &mut UploadWindow) -> &mut Check {
    &mut view.check
}

impl Shell {
    /// Brings Upload Avatar Item forward, opening it if it isn't; a given
    /// source (and kind) replaces the one being uploaded.
    pub(crate) fn open_ugc_upload(
        &mut self,
        source: Option<ItemSource>,
        kind: Option<ItemKind>,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self.ugc_upload {
            let raised = existing.update(cx, |root, window, cx| {
                window.activate_window();
                let (Some(source), Ok(view)) = (
                    source.clone(),
                    crate::window_blur::content(root.view(), cx).downcast::<UploadWindow>(),
                ) else {
                    return;
                };
                view.update(cx, |view, cx| {
                    if let Some(kind) = kind {
                        view.check.kind = kind;
                    }
                    view.set_source(source, window, cx);
                });
            });
            if raised.is_ok() {
                return;
            }
        }
        // Deferred: the window's first render reads this `Shell`, which is
        // still being updated here.
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = UploadWindow::open(shell.clone(), source, kind, cx);
            shell.update(cx, |shell, _| shell.ugc_upload = opened);
        });
    }
}

impl UploadWindow {
    fn open(
        shell: Entity<Shell>,
        source: Option<ItemSource>,
        kind: Option<ItemKind>,
        cx: &mut App,
    ) -> Option<WindowHandle<Root>> {
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
                title: Some("Upload Avatar Item".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| UploadWindow::new(shell, source, kind, window, cx));
            cx.new(|cx| Root::new(crate::window_blur::wrap(view, cx), window, cx))
        })
        .ok()
    }

    fn new(
        shell: Entity<Shell>,
        source: Option<ItemSource>,
        kind: Option<ItemKind>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (source, kind) = {
            let s = shell.read(cx);
            let source = source.or_else(|| Check::selection(s));
            let kind = kind
                .or_else(|| source.as_ref().and_then(|src| Check::guess_kind(s, src)))
                .unwrap_or_default();
            (source, kind)
        };
        let label = source.as_ref().map(ItemSource::label).unwrap_or_default();
        let title = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Item name");
            state.set_value(label, window, cx);
            state
        });
        let description =
            cx.new(|cx| InputState::new(window, cx).placeholder("Description (optional)"));
        let refresh =
            |_: &mut Self, _: Entity<InputState>, ev: &InputEvent, cx: &mut Context<Self>| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            };
        let subscriptions = vec![cx.subscribe(&title, refresh)];
        let mut view = UploadWindow {
            shell,
            check: Check::new(source, kind),
            scroll: ScrollHandle::new(),
            title,
            description,
            creator: Creator::Me,
            groups: crate::launcher::cached_groups(),
            phase: Phase::Idle,
            _subscriptions: subscriptions,
        };
        view.restart(cx);
        view
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        let shell = self.shell.clone();
        if !matches!(self.phase, Phase::Sending) {
            self.phase = Phase::Idle;
        }
        self.check.start(&shell, cx, slot);
        cx.notify();
    }

    fn set_kind(&mut self, kind: ItemKind, cx: &mut Context<Self>) {
        if self.check.kind != kind {
            self.check.kind = kind;
            self.restart(cx);
        }
    }

    /// Swaps the source, carrying the title along when it was still the
    /// old source's name.
    fn set_source(&mut self, source: ItemSource, window: &mut Window, cx: &mut Context<Self>) {
        let old = self
            .check
            .source
            .as_ref()
            .map(ItemSource::label)
            .unwrap_or_default();
        let current = self.title.read(cx).value().to_string();
        if current.trim().is_empty() || current == old {
            let label = source.label();
            self.title
                .update(cx, |s, cx| s.set_value(label, window, cx));
        }
        self.check.source = Some(source);
        self.restart(cx);
    }

    fn use_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (source, kind) = {
            let s = self.shell.read(cx);
            let Some(source) = Check::selection(s) else {
                return;
            };
            let kind = Check::guess_kind(s, &source);
            (source, kind)
        };
        if let Some(kind) = kind {
            self.check.kind = kind;
        }
        self.set_source(source, window, cx);
    }

    fn choose_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Upload".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |view, window, cx| {
                view.set_source(ItemSource::File(path), window, cx);
            });
        })
        .detach();
    }

    fn title_text(&self, cx: &App) -> String {
        self.title.read(cx).value().trim().to_string()
    }

    fn can_upload(&self, cx: &App) -> bool {
        self.check.source.is_some()
            && matches!(self.check.run, super::check::Run::Done(_))
            && !self.check.blocking()
            && !matches!(self.phase, Phase::Sending)
            && !self.title_text(cx).is_empty()
    }

    fn upload(&mut self, cx: &mut Context<Self>) {
        if !self.can_upload(cx) {
            return;
        }
        let Some(source) = self.check.source.clone() else {
            return;
        };
        let title = self.title_text(cx);
        let description = self.description.read(cx).value().trim().to_string();
        let creator = self.creator;
        let prepare: Box<dyn FnOnce() -> Result<Payload, String> + Send> = match source {
            ItemSource::Instance { root, .. } => {
                let shell = self.shell.read(cx);
                let model = super::super::clipboard::detached(&shell.dom, &[root]);
                match rbx_binary::serialize(&model) {
                    Ok(bytes) => Box::new(move || Ok(Payload::Rbxm(bytes))),
                    Err(e) => {
                        self.phase =
                            Phase::Failed(format!("Couldn\u{2019}t save the item as a model: {e}"));
                        cx.notify();
                        return;
                    }
                }
            }
            ItemSource::File(path) => Box::new(move || {
                rbx_import::prepare(&path).map(|(_, upload)| Payload::File(upload))
            }),
        };
        self.phase = Phase::Sending;
        cx.notify();
        let task = cx.background_spawn(async move {
            let payload = prepare()?;
            upload::send(&payload, &title, &description, creator)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |view, cx| {
                view.phase = match result {
                    Ok(id) => {
                        cx.open_url(&upload::creator_hub_url(id));
                        Phase::Done(id)
                    }
                    Err(msg) => Phase::Failed(msg),
                };
                cx.notify();
            });
        })
        .detach();
    }
}
