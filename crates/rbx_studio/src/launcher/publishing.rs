//! Roblox publishing: the stored key's
//! status and permission table, Check again, Replace key (the wizard's
//! paste-and-check flow in place of the table, saved on Continue) and
//! Remove key (after a confirmation).

use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rbx_cloud::ApiKey;

use super::key_check::{self, KeyCheck, Status};
use super::ui::{self, Weight};
use crate::tokens;

pub(super) struct Publishing {
    current: Entity<KeyCheck>,
    /// `Some` while Replace key is open.
    replacement: Option<Entity<KeyCheck>>,
    confirm_remove: bool,
    error: Option<String>,
    on_change: Rc<dyn Fn(&mut App)>,
    _observe: Vec<Subscription>,
}

/// `RBX_STUDIO_LAUNCHER_PUBLISHING=remove|replace`: open on the remove
/// confirmation or the replace flow, for a capture.
const STATE_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_PUBLISHING";

impl Publishing {
    pub(super) fn new(
        on_change: Rc<dyn Fn(&mut App)>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let secret = ApiKey::from_env_or_config()
            .map(|key| key.expose_secret().to_string())
            .or_else(|| {
                std::env::var(key_check::FIXTURE_VARIABLE)
                    .ok()
                    .map(|_| "x".repeat(964))
            })
            .unwrap_or_default();
        let current = cx.new(|cx| KeyCheck::with_key(secret, cx));
        let observe = vec![cx.observe(&current, |_, _, cx| cx.notify())];
        let mut this = Publishing {
            current,
            replacement: None,
            confirm_remove: false,
            error: None,
            on_change,
            _observe: observe,
        };
        match std::env::var(STATE_VARIABLE).as_deref() {
            Ok("remove") => this.confirm_remove = true,
            Ok("replace") => this.start_replace(cx),
            _ => {}
        }
        this
    }

    fn start_replace(&mut self, cx: &mut Context<Self>) {
        let replacement = cx.new(KeyCheck::new);
        self._observe
            .push(cx.observe(&replacement, |_, _, cx| cx.notify()));
        self.replacement = Some(replacement);
        cx.notify();
    }

    fn save_replacement(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.replacement.as_ref().and_then(|r| r.read(cx).key()) else {
            return;
        };
        let secret = key.expose_secret().to_string();
        cx.spawn(async move |this, cx| {
            let result = crate::key_store::save(key, cx).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.current = cx.new(|cx| KeyCheck::with_key(secret, cx));
                        let current = this.current.clone();
                        this._observe
                            .push(cx.observe(&current, |_, _, cx| cx.notify()));
                        this.replacement = None;
                        this.error = None;
                        (this.on_change)(cx);
                    }
                    Err(err) => {
                        this.error = Some(format!(
                            "The key couldn\u{2019}t be stored in your system keychain: {err}"
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = crate::key_store::forget(cx).await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => {
                    (this.on_change)(cx);
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
                Err(err) => {
                    this.confirm_remove = false;
                    this.error = Some(format!(
                        "The key couldn\u{2019}t be removed from your system keychain: {err}"
                    ));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn status_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.current.read(cx);
        let (name, tag, meta): (SharedString, Option<Div>, SharedString) = match &state.status {
            Status::Done(checked) => {
                let ready = checked.report.ready();
                let expiry = if checked.info.expiration_time_utc.is_empty() {
                    "never expires".to_string()
                } else {
                    format!(
                        "expires {}",
                        key_check::short_date(&checked.info.expiration_time_utc)
                    )
                };
                (
                    checked.info.name.clone().into(),
                    Some(if ready {
                        ui::tag("READY", ui::green(), ui::green_soft())
                    } else {
                        ui::tag("NEEDS ATTENTION", ui::red(), ui::red_soft())
                    }),
                    format!("{} \u{b7} {expiry} \u{b7} checked just now", checked.owner).into(),
                )
            }
            Status::Running => (
                "Your key".into(),
                None,
                "Checking with Roblox\u{2026}".into(),
            ),
            Status::Idle => (
                "No key stored".into(),
                None,
                "Replace key to add one.".into(),
            ),
            Status::Invalid(status) => (
                "Your key".into(),
                Some(ui::tag("REFUSED", ui::red(), ui::red_soft())),
                format!("Roblox answered {status}. Replace it with a working key.").into(),
            ),
            Status::Network => (
                "Your key".into(),
                None,
                "Couldn\u{2019}t reach Roblox. Check your connection and try again.".into(),
            ),
        };
        let current = self.current.clone();
        v_flex()
            .gap(px(14.))
            .p(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .child(
                h_flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        h_flex()
                            .size(px(36.))
                            .flex_none()
                            .rounded(px(8.))
                            .bg(tokens::accent_soft())
                            .text_color(ui::accent())
                            .items_center()
                            .justify_center()
                            .child(ui::icon("key-round", 17.)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(1.))
                            .child(
                                h_flex()
                                    .gap(px(8.))
                                    .items_center()
                                    .child(
                                        ui::text(13., 18.)
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(tokens::text())
                                            .child(name),
                                    )
                                    .children(tag),
                            )
                            .child(
                                ui::text(11.5, 16.)
                                    .truncate()
                                    .text_color(tokens::text2())
                                    .child(meta),
                            ),
                    )
                    .child(
                        ui::icon_button(
                            "publishing-check",
                            "refresh-cw",
                            "Check again",
                            Weight::Secondary,
                            true,
                        )
                        .on_click(move |_, _, cx| current.update(cx, |this, cx| this.run(cx))),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(tokens::border())
                    .text_color(tokens::text3())
                    .child(ui::icon("shield-check", 13.))
                    .child(
                        ui::text(11.5, 16.)
                            .flex_1()
                            .child("Stored encrypted in your system keychain."),
                    )
                    .child(
                        ui::icon_button(
                            "publishing-remove",
                            "trash",
                            "Remove key",
                            Weight::Secondary,
                            true,
                        )
                        .text_color(ui::red())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_remove = true;
                            cx.notify();
                        })),
                    )
                    .child(
                        ui::button("publishing-replace", "Replace key", Weight::Secondary, true)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.replacement.is_some() {
                                    this.replacement = None;
                                    cx.notify();
                                } else {
                                    this.start_replace(cx);
                                }
                            })),
                    ),
            )
    }

    fn lower(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(replacement) = &self.replacement {
            let ready = replacement.read(cx).key().is_some();
            return v_flex()
                .flex_1()
                .min_h_0()
                .gap(px(14.))
                .child(key_check::field(replacement, window, cx))
                .when(matches!(replacement.read(cx).status, Status::Idle), |this| {
                    this.child(ui::text(12., 17.).text_color(tokens::text3()).child(
                        "RbxNative checks the new key first. The current one stays until you press Continue.",
                    ))
                })
                .children(key_check::result(replacement, None, cx))
                .child(div().flex_1())
                .child(
                    h_flex()
                        .gap(px(8.))
                        .justify_end()
                        .child(
                            ui::button("replace-cancel", "Cancel", Weight::Secondary, false)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.replacement = None;
                                    cx.notify();
                                })),
                        )
                        .child(if ready {
                            ui::button("replace-continue", "Continue", Weight::Primary, false)
                                .min_w(px(112.))
                                .on_click(cx.listener(|this, _, _, cx| this.save_replacement(cx)))
                                .into_any_element()
                        } else {
                            ui::disabled_button("replace-continue", "Continue")
                                .min_w(px(112.))
                                .into_any_element()
                        }),
                )
                .into_any_element();
        }
        match &self.current.read(cx).status {
            Status::Done(checked) if checked.report.usable => {
                key_check::table(checked, None, &self.current.read(cx).scroll).into_any_element()
            }
            _ => key_check::result(&self.current, None, cx)
                .unwrap_or_else(|| div().into_any_element()),
        }
    }
}

impl Render for Publishing {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog = self.confirm_remove.then(|| {
            ui::dialog(
                460.,
                ui::dialog_glyph("trash", ui::red(), ui::red_soft()),
                "Remove this key from RbxNative?",
                "It\u{2019}s deleted from your system keychain and the setup wizard opens next time. The key keeps working on Roblox until you revoke it on the Creator Dashboard.",
                None,
                vec![
                    ui::button("remove-cancel", "Cancel", Weight::Secondary, false)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_remove = false;
                            cx.notify();
                        }))
                        .into_any_element(),
                    ui::button("remove-confirm", "Remove key", Weight::Danger, false)
                        .on_click(cx.listener(|this, _, window, cx| this.remove(window, cx)))
                        .into_any_element(),
                ],
            )
        });
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(ui::panel())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_color(tokens::text())
            .text_size(px(13.))
            .child(crate::shell::chrome::window_topbar("Roblox publishing".into(), true,
                |window, _| window.remove_window(),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(18.))
                    .pt(px(28.))
                    .px(px(32.))
                    .pb(px(24.))
                    .child(
                        v_flex()
                            .gap(px(4.))
                            .child(
                                ui::text(20., 28.)
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(tokens::text())
                                    .child("Roblox publishing"),
                            )
                            .child(ui::text(12.5, 19.).text_color(tokens::text2()).child(
                                "The Open Cloud key RbxNative uses to open, save and publish your places.",
                            )),
                    )
                    .child(self.status_card(cx))
                    .children(self.error.clone().map(|error| {
                        ui::text(11.5, 16.).text_color(ui::red()).child(error)
                    }))
                    .child(self.lower(window, cx)),
            )
            .children(dialog)
    }
}
