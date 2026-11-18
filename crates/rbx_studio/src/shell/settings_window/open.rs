//! Opening Settings, one window at a time. From Home the window comes up at
//! once with its title bar, and its pages fill in once the hidden editor
//! behind them is built (see `main::open_settings_from_home`).

use gpui_kit::component::{v_flex, Root};
use gpui_kit::*;

use crate::tokens;

use super::super::Shell;
use super::{SettingsWindow, ThemeInstall};

impl Shell {
    /// Brings the Settings window forward, opening it if it isn't.
    /// Opens Settings on the page named `key` (as `Page::from_key` reads
    /// it). A window already open is only brought forward.
    pub(crate) fn open_settings_on(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.settings_open_on = Some(key);
        self.open_settings(cx);
    }

    pub(crate) fn settings_window(&self) -> Option<AnyWindowHandle> {
        self.settings_window.map(Into::into)
    }

    pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
        if self.settings_opening {
            return;
        }
        // Home's Settings, opened while this place loaded, gives way to
        // this editor's, which applies changes to it live.
        crate::close_home_settings(cx.entity_id(), cx);
        if let Some(existing) = self.settings_window {
            if bring_forward(existing.into(), cx) {
                return;
            }
        }
        // An install error belongs to the window it was shown in.
        if matches!(self.theme_install, ThemeInstall::Failed(_)) {
            self.theme_install = ThemeInstall::Idle;
        }
        // Deferred: opening a window renders it at once, and its first
        // render reads this `Shell`, which is still being updated here.
        self.settings_opening = true;
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = cx
                .open_window(SettingsWindow::options(cx), |window, cx| {
                    let view = SettingsWindow::build(shell.clone(), window, cx);
                    cx.new(|cx| Root::new(crate::window_blur::wrap(view, cx), window, cx))
                })
                .ok();
            shell.update(cx, |shell, _| {
                shell.settings_window = opened;
                shell.settings_opening = false;
            });
        });
    }
}

/// Activates `window`; false once it has closed. A key typed in that very
/// window (Alt+S with Settings focused) is handled while the window is
/// being updated, when updating it again fails although it is open, so the
/// active window counts as already forward.
pub(crate) fn bring_forward(window: AnyWindowHandle, cx: &mut App) -> bool {
    cx.active_window() == Some(window)
        || window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
}

/// The window's content: the title bar alone until [`fill_loading`] gives
/// it the pages.
pub(crate) struct Loading {
    content: Option<AnyView>,
}

impl Render for Loading {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if let Some(content) = &self.content {
            return content.clone().into_any_element();
        }
        v_flex()
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .text_size(px(13.))
            .text_color(tokens::text_muted())
            .child(super::super::chrome::window_topbar(
                "Settings".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("Loading settings…"),
            )
            .into_any_element()
    }
}

/// Opens the Settings window before the editor it shows exists.
pub(crate) fn open_loading(cx: &mut App) -> Option<WindowHandle<Root>> {
    cx.open_window(SettingsWindow::options(cx), |window, cx| {
        let loading = cx.new(|_| Loading { content: None });
        cx.new(|cx| Root::new(crate::window_blur::wrap(loading, cx), window, cx))
    })
    .ok()
}

/// Fills a window [`open_loading`] opened with `shell`'s Settings; false
/// when it was closed meanwhile.
pub(crate) fn fill_loading(handle: WindowHandle<Root>, shell: Entity<Shell>, cx: &mut App) -> bool {
    let filled = handle
        .update(cx, |root, window, cx| {
            let content = crate::window_blur::content(root.view(), cx);
            let Ok(loading) = content.downcast::<Loading>() else {
                return false;
            };
            let view = SettingsWindow::build(shell.clone(), window, cx);
            loading.update(cx, |loading, cx| {
                loading.content = Some(view.into());
                cx.notify();
            });
            true
        })
        .unwrap_or(false);
    if filled {
        // Notifying the view alone left the window on "Loading settings…"
        // on Windows until the pointer moved over it: redraw it whole.
        let _ = handle.update(cx, |_, window, _| window.refresh());
        shell.update(cx, |shell, _| shell.settings_window = Some(handle));
    }
    filled
}
