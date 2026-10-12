//! The Animation document's confirmations: one short line over the top of
//! the 3D view, in the viewport's own speed-readout style, instead of a status
//! line at the bottom of a long side panel nobody scrolls to. Info and
//! warnings fade on their own; an error stays until it is dismissed (or the
//! next message replaces it), so a failed import or publish is not missed.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Animation, Shell};

/// How long an info line stays: long enough to read a dozen words twice.
const INFO: Duration = Duration::from_secs(4);
/// A warning asks the user to do something first, so it stays a little longer.
const WARNING: Duration = Duration::from_secs(7);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::shell) enum ToastKind {
    Info,
    Warning,
    Error,
}

pub(in crate::shell) struct Toast {
    pub(in crate::shell) text: SharedString,
    pub(in crate::shell) kind: ToastKind,
    /// Tells this toast from the one that replaced it, so an older toast's
    /// timer never clears a newer one.
    serial: u64,
    shown: Instant,
    /// Whether this toast's fade timer is running.
    armed: bool,
}

impl Toast {
    fn lifetime(&self) -> Option<Duration> {
        match self.kind {
            ToastKind::Info => Some(INFO),
            ToastKind::Warning => Some(WARNING),
            ToastKind::Error => None,
        }
    }
}

impl Animation {
    fn show(&mut self, kind: ToastKind, text: SharedString) {
        let serial = self.toast.as_ref().map_or(0, |t| t.serial + 1);
        self.toast = Some(Toast {
            text,
            kind,
            serial,
            shown: Instant::now(),
            armed: false,
        });
    }

    /// A confirmation: something happened as asked.
    pub(in crate::shell) fn say(&mut self, text: impl Into<SharedString>) {
        self.show(ToastKind::Info, text.into());
    }

    /// Something the user has to do first (pick a joint, select a key).
    pub(in crate::shell) fn warn(&mut self, text: impl Into<SharedString>) {
        self.show(ToastKind::Warning, text.into());
    }

    /// A failure, which stays until dismissed.
    pub(in crate::shell) fn fail(&mut self, text: impl Into<SharedString>) {
        self.show(ToastKind::Error, text.into());
    }

    /// The text currently shown, for tests and the scripted run.
    #[allow(dead_code)]
    pub(in crate::shell) fn toast_text(&self) -> Option<&str> {
        self.toast.as_ref().map(|t| t.text.as_ref())
    }
}

impl Shell {
    /// The toast over the 3D view, if one is up; arms its fade on first show.
    pub(super) fn animation_toast(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let toast = self.anim.toast.as_mut()?;
        if let Some(lifetime) = toast.lifetime() {
            let left = lifetime.saturating_sub(toast.shown.elapsed());
            if left.is_zero() {
                self.anim.toast = None;
                return None;
            }
            if !toast.armed {
                toast.armed = true;
                let serial = toast.serial;
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(left).await;
                    let _ = this.update(cx, |shell, cx| {
                        if shell.anim.toast.as_ref().map(|t| t.serial) == Some(serial) {
                            shell.anim.toast = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
        let (kind, text) = (toast.kind, toast.text.clone());
        let (glyph, tint) = match kind {
            ToastKind::Info => (IconName::CircleCheck, rgb(0x8fd19e)),
            ToastKind::Warning => (IconName::TriangleAlert, rgb(0xf0c674)),
            ToastKind::Error => (IconName::CircleX, rgb(0xf38b8b)),
        };
        Some(
            div()
                .absolute()
                .top_2()
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    h_flex()
                        .id("anim-toast")
                        .debug_selector(|| "anim-toast".into())
                        .max_w(px(560.))
                        .gap(px(6.))
                        .items_center()
                        .px_2()
                        .py_0p5()
                        .bg(rgba(0x14151ae0))
                        .text_size(crate::tokens::viewport_text(12.))
                        .text_color(rgb(0xe4e5e9))
                        .child(Icon::new(glyph).size(px(12.)).text_color(tint))
                        .child(div().child(text))
                        .when(kind == ToastKind::Error, |this| {
                            this.child(
                                div()
                                    .id("anim-toast-close")
                                    .debug_selector(|| "anim-toast-close".into())
                                    .ml(px(4.))
                                    .cursor_pointer()
                                    .text_color(rgb(0x9a9ca5))
                                    .hover(|s| s.text_color(rgb(0xe4e5e9)))
                                    .child(Icon::new(IconName::X).size(px(12.)))
                                    .tooltip(|window, cx| {
                                        crate::shell::tooltip::text("Dismiss", window, cx)
                                    })
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        shell.anim.toast = None;
                                        cx.notify();
                                    })),
                            )
                        }),
                )
                .into_any_element(),
        )
    }
}
