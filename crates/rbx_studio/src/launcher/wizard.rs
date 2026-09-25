//! The first-launch setup wizard: Welcome, Create a
//! key (the Dashboard walk-through with its three-slide carousel), Paste
//! and check, Done. Skip for now goes to Home without a key.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::key_check::{self, KeyCheck, Status};
use super::ui::{self};
use super::Boot;
use crate::tokens;

mod create;
mod frame;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Step {
    Welcome,
    Create,
    Paste,
    Done,
}

impl Step {
    pub(super) const ALL: [Step; 4] = [Step::Welcome, Step::Create, Step::Paste, Step::Done];

    pub(super) fn labels(self) -> (&'static str, &'static str) {
        match self {
            Step::Welcome => ("Welcome", "Why RbxNative needs a key"),
            Step::Create => ("Create a key", "On the Creator Dashboard"),
            Step::Paste => ("Paste and check", "See what the key can do"),
            Step::Done => ("Done", "Open your games"),
        }
    }
}

/// `RBX_STUDIO_LAUNCHER_STEP=welcome|create|create2|create3|paste|done`:
/// the step (and carousel slide) the wizard opens on, for a capture.
const STEP_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_STEP";

pub(super) struct Wizard {
    pub(super) boot: Boot,
    pub(super) step: Step,
    pub(super) slide: usize,
    pub(super) check: Entity<KeyCheck>,
    pub(super) saving: bool,
    pub(super) save_error: Option<String>,
    _observe: Subscription,
}

impl Wizard {
    pub(super) fn new(boot: Boot, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let check = cx.new(KeyCheck::new);
        let observe = cx.observe(&check, |_, _, cx| cx.notify());
        let (step, slide) = match std::env::var(STEP_VARIABLE).as_deref() {
            Ok("create") => (Step::Create, 0),
            Ok("create2") => (Step::Create, 1),
            Ok("create3") => (Step::Create, 2),
            Ok("paste") => (Step::Paste, 0),
            Ok("done") => (Step::Done, 0),
            _ => (Step::Welcome, 0),
        };
        if step >= Step::Paste && std::env::var(key_check::FIXTURE_VARIABLE).is_ok() {
            check.update(cx, |check, cx| {
                check.set_fixture_secret();
                check.run(cx);
            });
        }
        Wizard {
            boot,
            step,
            slide,
            check,
            saving: false,
            save_error: None,
            _observe: observe,
        }
    }

    /// Hands over to Home, with or without a key, and closes the wizard.
    pub(super) fn go_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        super::open_home(self.boot.clone(), cx);
        window.remove_window();
    }

    pub(super) fn save_and_continue(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.check.read(cx).key() else {
            return;
        };
        if std::env::var(key_check::FIXTURE_VARIABLE).is_ok() {
            self.step = Step::Done;
            cx.notify();
            return;
        }
        self.saving = true;
        self.save_error = None;
        cx.notify();
        let session_key = key.clone();
        cx.spawn(async move |this, cx| {
            let result = crate::key_store::save(key, cx).await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => this.step = Step::Done,
                    // No keychain to write to (no session bus, no Secret
                    // Service): the key still works until the app closes,
                    // and setup opens again next launch. Never a plaintext
                    // fallback.
                    Err(err) => {
                        rbx_cloud::ApiKey::install(Some(session_key));
                        this.save_error = Some(format!(
                            "Your system keychain couldn\u{2019}t be reached, so the key works until RbxNative closes and setup opens again next launch. ({err})"
                        ));
                        this.step = Step::Done;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn heading(
        title: &'static str,
        text: &'static str,
        action: Option<AnyElement>,
    ) -> impl IntoElement {
        h_flex()
            .items_start()
            .gap(px(16.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .child(
                        ui::text(20., 28.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text())
                            .child(title),
                    )
                    .child(ui::text(12.5, 19.).text_color(tokens::text2()).child(text)),
            )
            .children(action)
    }

    pub(super) fn big_glyph(
        glyph: &'static str,
        fg: Rgba,
        fill: Rgba,
        round: bool,
    ) -> impl IntoElement {
        h_flex()
            .size(px(48.))
            .flex_none()
            .rounded(if round { px(24.) } else { px(12.) })
            .bg(fill)
            .text_color(fg)
            .items_center()
            .justify_center()
            .child(ui::icon(glyph, 22.))
    }

    pub(super) fn feature_card(
        glyph: &'static str,
        title: SharedString,
        text: SharedString,
    ) -> impl IntoElement {
        h_flex()
            .items_start()
            .gap(px(12.))
            .py(px(14.))
            .px(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .child(
                h_flex()
                    .size(px(32.))
                    .flex_none()
                    .rounded(px(7.))
                    .bg(tokens::accent_soft())
                    .text_color(ui::accent())
                    .items_center()
                    .justify_center()
                    .child(ui::icon(glyph, 16.)),
            )
            .child(
                v_flex()
                    .gap(px(2.))
                    .child(
                        ui::text(12.5, 17.)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens::text())
                            .child(title),
                    )
                    .child(ui::text(12., 18.).text_color(tokens::text2()).child(text)),
            )
    }

    fn welcome(&self) -> AnyElement {
        v_flex()
            .gap(px(18.))
            .child(Self::big_glyph("cloud-upload", ui::accent(), tokens::accent_soft(), false))
            .child(Self::heading(
                "Connect RbxNative to Roblox",
                "RbxNative opens, saves and publishes your places through Roblox Open Cloud. For that it needs an API key from your Creator Dashboard. You make it once, here, step by step.",
                None,
            ))
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(Self::feature_card("gamepad-2", "Open your games".into(), "Your experiences show up on Home. Pick one and it opens in the editor.".into()))
                    .child(Self::feature_card("cloud-upload", "Save and publish".into(), "Send your place back to Roblox and publish updates without opening Roblox Studio.".into()))
                    .child(Self::feature_card("shield-check", "Your key, your permissions".into(), "You choose exactly what the key can do. RbxNative never sees your Roblox password.".into()))
                    .child(Self::feature_card("file", "Local files don\u{2019}t need it".into(), "Opening and saving .rbxl and .rbxlx files works with or without a key.".into())),
            )
            .into_any_element()
    }

    fn paste(&self, window: &Window, cx: &App) -> AnyElement {
        let table_height = match &self.check.read(cx).status {
            Status::Done(checked) if !checked.report.ready() => 350.,
            _ => 330.,
        };
        let ready = matches!(&self.check.read(cx).status, Status::Done(c) if c.report.ready());
        v_flex()
            .gap(px(18.))
            .child(Self::heading(
                "Paste your key",
                "RbxNative checks it with Roblox and shows what it can do. Nothing is saved until you continue.",
                None,
            ))
            .child(key_check::field(&self.check, window, cx))
            .children(key_check::result(&self.check, Some(table_height), cx))
            .when(ready, |this| {
                this.child(ui::text(11.5, 17.).text_color(tokens::text3()).child(
                    "Optional permissions only switch features on. Add them to the key on the Creator Dashboard any time, then check again.",
                ))
            })
            .into_any_element()
    }

    fn done(&self, cx: &App) -> AnyElement {
        let summary = match &self.check.read(cx).status {
            Status::Done(checked) => {
                let ((req_on, req_all), (opt_on, opt_all)) = key_check::counts(&checked.report);
                let expiry = if checked.info.expiration_time_utc.is_empty() {
                    "Never expires.".to_string()
                } else {
                    format!(
                        "Expires {}.",
                        key_check::short_date(&checked.info.expiration_time_utc)
                    )
                };
                (
                    format!(
                        "{} \u{b7} {req_on} of {req_all} required, {opt_on} of {opt_all} optional",
                        checked.info.name
                    ),
                    format!("Owned by {}. {expiry}", checked.owner),
                )
            }
            _ => (String::new(), String::new()),
        };
        v_flex()
            .gap(px(18.))
            .child(Self::big_glyph("check", ui::green(), ui::green_soft(), true))
            .child(Self::heading(
                "You\u{2019}re all set",
                "Home lists your experiences now. Pick one and it opens in the editor; Save and Publish send it back to Roblox.",
                None,
            ))
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(Self::feature_card("key-round", summary.0.into(), summary.1.into()))
                    .child(match &self.save_error {
                        None => Self::feature_card(
                            "shield-check",
                            "Stored encrypted in your system keychain".into(),
                            "Your IP restriction and expiration are what make a copied key useless, so keep them on.".into(),
                        ),
                        Some(error) => Self::feature_card(
                            "triangle-alert",
                            "Not stored: this session only".into(),
                            error.clone().into(),
                        ),
                    })
                    .child(Self::feature_card(
                        "refresh-cw",
                        "Change it any time".into(),
                        "Click your name at the bottom of Home to check the key again, replace it or remove it.".into(),
                    )),
            )
            .into_any_element()
    }
}

impl Render for Wizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.step {
            Step::Welcome => self.welcome(),
            Step::Create => self.create(cx),
            Step::Paste => self.paste(window, cx),
            Step::Done => self.done(cx),
        };
        v_flex()
            .size_full()
            .bg(ui::panel())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_color(tokens::text())
            .text_size(px(13.))
            .child(crate::shell::chrome::window_topbar(
                "Set up publishing".into(),
                false,
                |_, cx| cx.quit(),
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.rail())
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                v_flex()
                                    .id("wizard-body")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .pt(px(28.))
                                    .px(px(32.))
                                    .pb(px(20.))
                                    .child(body),
                            )
                            .child(self.footer(cx)),
                    ),
            )
    }
}
