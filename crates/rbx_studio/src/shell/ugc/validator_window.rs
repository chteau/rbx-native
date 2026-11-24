//! Avatar › Check Item: the validator window over the selection or a file.

use gpui_kit::component::Root;
use gpui_kit::*;

use super::super::Shell;
use super::check::{Check, Run};
use super::{visual, ItemKind, ItemSource};
use crate::tokens;

mod view;

const WIDTH: f32 = 780.;
const HEIGHT: f32 = 660.;
const MIN_WIDTH: f32 = 620.;
const MIN_HEIGHT: f32 = 480.;

struct ValidatorWindow {
    shell: Entity<Shell>,
    check: Check,
    scroll: ScrollHandle,
    /// The finding drawn on the model (Visualizing UGC Validation), by its
    /// index in the findings.
    shown: Option<usize>,
    /// Where the shown finding was last drawn, so a redraw only happens
    /// when its part moved.
    drawn_at: Option<glam::Mat4>,
    _subscriptions: Vec<Subscription>,
}

fn slot(view: &mut ValidatorWindow) -> &mut Check {
    &mut view.check
}

impl Shell {
    /// Brings Check Avatar Item forward, opening it if it isn't; a given
    /// source replaces the one being checked.
    pub(crate) fn open_ugc_validator(
        &mut self,
        source: Option<ItemSource>,
        kind: Option<ItemKind>,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self.ugc_validator {
            let raised = existing.update(cx, |root, window, cx| {
                window.activate_window();
                let Some(view) = crate::window_blur::content(root.view(), cx)
                    .downcast::<ValidatorWindow>()
                    .ok()
                else {
                    return;
                };
                if let Some(source) = source.clone() {
                    view.update(cx, |view, cx| {
                        view.check.source = Some(source);
                        if let Some(kind) = kind {
                            view.check.kind = kind;
                        }
                        view.restart(cx);
                    });
                }
            });
            if raised.is_ok() {
                return;
            }
        }
        // Deferred: the window's first render reads this `Shell`, which is
        // still being updated here.
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = ValidatorWindow::open(shell.clone(), source, kind, cx);
            shell.update(cx, |shell, _| shell.ugc_validator = opened);
        });
    }
}

impl ValidatorWindow {
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
                title: Some("Check Avatar Item".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| ValidatorWindow::new(shell, source, kind, cx));
            cx.new(|cx| Root::new(crate::window_blur::wrap(view, cx), window, cx))
        })
        .ok()
    }

    fn new(
        shell: Entity<Shell>,
        source: Option<ItemSource>,
        kind: Option<ItemKind>,
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
        // The drawing follows the item as it moves and goes with the window.
        let observe = cx.observe(&shell, |view: &mut ValidatorWindow, _, cx| view.redraw(cx));
        let release = cx.on_release(|view: &mut ValidatorWindow, cx| {
            if view.shown.is_some() {
                view.shell.update(cx, |shell, cx| {
                    shell
                        .viewport
                        .update(cx, |v, _| v.set_ugc_validation(Vec::new()))
                });
            }
        });
        let mut view = ValidatorWindow {
            shell,
            check: Check::new(source, kind),
            scroll: ScrollHandle::new(),
            shown: None,
            drawn_at: None,
            _subscriptions: vec![observe, release],
        };
        view.restart(cx);
        view
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.clear(cx);
        let shell = self.shell.clone();
        self.check.start(&shell, cx, slot);
        cx.notify();
    }

    /// Whether findings can be clicked to draw them: the beta is on and the
    /// item is in the place, not a file.
    fn picking(&self) -> bool {
        crate::beta::enabled(visual::BETA_ID)
            && matches!(self.check.source, Some(ItemSource::Instance { .. }))
    }

    /// Draws finding `index` on the model, as clicking a check's row does in
    /// Studio: another row replaces the drawing, and one with nothing to
    /// point at clears it.
    fn show(&mut self, index: usize, cx: &mut Context<Self>) {
        self.shown = Some(index);
        self.drawn_at = None;
        self.redraw(cx);
        cx.notify();
    }

    fn redraw(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.shown else { return };
        let picked = match &self.check.run {
            Run::Done(done) => done
                .findings
                .get(index)
                .filter(|f| visual::drawable(f.rule))
                .and_then(|f| {
                    let at = visual::frame(&self.shell.read(cx).dom, &done.collected, f)?;
                    Some((at, f, &done.collected))
                }),
            _ => None,
        };
        let at = picked.as_ref().map(|p| p.0);
        if at.is_some() && at == self.drawn_at {
            return;
        }
        self.drawn_at = at;
        let segments = picked.map_or_else(Vec::new, |(at, f, collected)| {
            visual::drawing(collected, f, at)
        });
        self.shell.update(cx, |shell, cx| {
            shell
                .viewport
                .update(cx, |v, _| v.set_ugc_validation(segments))
        });
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.drawn_at = None;
        if self.shown.take().is_some() {
            self.shell.update(cx, |shell, cx| {
                shell
                    .viewport
                    .update(cx, |v, _| v.set_ugc_validation(Vec::new()))
            });
        }
    }

    fn use_selection(&mut self, cx: &mut Context<Self>) {
        let s = self.shell.read(cx);
        let Some(source) = Check::selection(s) else {
            return;
        };
        if let Some(kind) = Check::guess_kind(s, &source) {
            self.check.kind = kind;
        }
        self.check.source = Some(source);
        self.restart(cx);
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Check".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                view.check.source = Some(ItemSource::File(path));
                view.restart(cx);
            });
        })
        .detach();
    }
}
