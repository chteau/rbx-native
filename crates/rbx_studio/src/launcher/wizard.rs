//! The first-launch setup wizard (boards `Setup-*`): Welcome, Create a
//! key (the Dashboard walk-through with its three-slide carousel), Paste
//! and check, Done. Skip for now goes to Home without a key.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::key_check::{self, KeyCheck, Status};
use super::ui::{self, Weight};
use super::Boot;
use crate::tokens;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    Welcome,
    Create,
    Paste,
    Done,
}

impl Step {
    const ALL: [Step; 4] = [Step::Welcome, Step::Create, Step::Paste, Step::Done];

    fn labels(self) -> (&'static str, &'static str) {
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
    boot: Boot,
    step: Step,
    slide: usize,
    check: Entity<KeyCheck>,
    saving: bool,
    save_error: Option<String>,
    grab: Rc<Cell<bool>>,
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
            grab: Rc::new(Cell::new(false)),
            _observe: observe,
        }
    }

    /// Hands over to Home, with or without a key, and closes the wizard.
    fn go_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        super::open_home(self.boot.clone(), cx);
        window.remove_window();
    }

    fn save_and_continue(&mut self, cx: &mut Context<Self>) {
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
        cx.spawn(async move |this, cx| {
            let result = crate::key_store::save(key, cx).await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => this.step = Step::Done,
                    Err(err) => {
                        this.save_error = Some(format!(
                            "The key couldn\u{2019}t be stored in your system keychain: {err}"
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn rail(&self) -> impl IntoElement {
        v_flex()
            .w(px(240.))
            .flex_none()
            .gap(px(4.))
            .pt(px(24.))
            .px(px(14.))
            .pb(px(20.))
            .bg(ui::bg())
            .border_r_1()
            .border_color(tokens::border())
            .child(
                v_flex()
                    .gap(px(2.))
                    .px(px(10.))
                    .pb(px(14.))
                    .child(
                        ui::text(14., 20.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text())
                            .child("Set up publishing"),
                    )
                    .child(ui::text(11.5, 16.).text_color(tokens::text2()).child("About three minutes")),
            )
            .children(Step::ALL.iter().enumerate().map(|(index, &step)| {
                let (title, sub) = step.labels();
                let current = step == self.step;
                let done = step < self.step;
                let marker = if done {
                    ui::status_dot("check", ui::green(), Some(ui::green_soft()), 22.)
                } else {
                    h_flex()
                        .size(px(22.))
                        .flex_none()
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .font_family(tokens::FONT_FAMILY_MONO)
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .map(|this| {
                            if current {
                                this.bg(ui::accent()).text_color(ui::bg()).font_weight(FontWeight::MEDIUM)
                            } else {
                                this.border_1().border_color(tokens::border2()).text_color(tokens::text3())
                            }
                        })
                        .child((index + 1).to_string())
                };
                h_flex()
                    .h(px(48.))
                    .items_center()
                    .gap(px(12.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .when(current, |this| this.bg(tokens::accent_soft()))
                    .child(marker)
                    .child(
                        v_flex()
                            .gap(px(1.))
                            .child(
                                ui::text(12.5, 17.)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if current {
                                        tokens::text()
                                    } else if done {
                                        tokens::text2()
                                    } else {
                                        tokens::text3()
                                    })
                                    .child(title),
                            )
                            .child(ui::text(11., 15.).text_color(tokens::text3()).child(sub)),
                    )
            }))
            .child(div().flex_1())
            .child(
                h_flex()
                    .items_start()
                    .gap(px(8.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(tokens::border())
                    .text_color(tokens::text2())
                    .child(ui::icon("info", 14.).flex_none())
                    .child(ui::text(11., 16.).flex_1().min_w_0().child(
                        "Roblox Studio signs in with your account. RbxNative can't, so it uses a key you create and can revoke at any time.",
                    )),
            )
    }

    fn heading(
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

    fn big_glyph(glyph: &'static str, fg: Rgba, fill: Rgba, round: bool) -> impl IntoElement {
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

    fn feature_card(
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

    fn create(&self, cx: &mut Context<Self>) -> AnyElement {
        const SLIDES: [(&str, &str); 3] = [
            (
                "Open API Keys and press Create API Key",
                "Creator Dashboard \u{2192} Credentials \u{2192} API Keys. Name it RbxNative.",
            ),
            (
                "Add the three required permissions",
                "universe-places \u{2192} Write, legacy-asset \u{2192} Manage, then Inventory \u{2192} Read.",
            ),
            (
                "Save & Generate, then copy the key",
                "Set Security (your IP, an expiration) first. Roblox shows the key once.",
            ),
        ];
        let slide = self.slide;
        let (title, sub) = SLIDES[slide];
        let arrow = |id: &'static str, glyph: &'static str, enabled: bool, left: bool| {
            h_flex()
                .id(id)
                .absolute()
                .top(px(76.))
                .when(left, |this| this.left(px(12.)))
                .when(!left, |this| this.right(px(12.)))
                .size(px(32.))
                .rounded_full()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(tokens::border2())
                .bg(ui::panel())
                .text_color(if enabled {
                    tokens::text()
                } else {
                    tokens::text3()
                })
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .hover(|this| this.bg(tokens::secondary_hover()))
                })
                .child(ui::icon(glyph, 16.))
        };
        let row = |label: &'static str, value: AnyElement, last: bool| {
            h_flex()
                .min_h(px(34.))
                .items_center()
                .gap(px(12.))
                .py(px(7.))
                .px(px(14.))
                .when(!last, |this| {
                    this.border_b_1().border_color(tokens::border())
                })
                .child(
                    ui::text(12., 18.)
                        .w(px(120.))
                        .flex_none()
                        .text_color(tokens::text2())
                        .child(label),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(tokens::text())
                        .child(value),
                )
        };
        let chip = |label: &'static str| {
            h_flex()
                .h(px(20.))
                .px(px(6.))
                .items_center()
                .rounded(px(4.))
                .border_1()
                .border_color(tokens::border())
                .bg(ui::panel())
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_size(px(11.))
                .line_height(px(16.))
                .child(label)
        };
        v_flex()
            .gap(px(18.))
            .child(Self::heading(
                "Create an API key",
                "Follow along on the Creator Dashboard. Keep this window open.",
                Some(
                    ui::external_button("wizard-dashboard", "Open Creator Dashboard", Weight::Secondary, false)
                        .on_click(|_, _, cx| cx.open_url(rbx_cloud::DASHBOARD_API_KEYS_URL))
                        .into_any_element(),
                ),
            ))
            .child(
                v_flex()
                    .flex_none()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(tokens::border())
                    .bg(ui::panel2())
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .h(px(184.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .py(px(14.))
                            .px(px(60.))
                            .bg(rgb(0x131314))
                            .border_b_1()
                            .border_color(tokens::border())
                            .children(slide_image(slide).map(|image| {
                                img(image).max_w_full().max_h_full().object_fit(ObjectFit::Contain).rounded(px(4.))
                            }))
                            .child(arrow("slide-prev", "chevron-left", slide > 0, true).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.slide = this.slide.saturating_sub(1);
                                    cx.notify();
                                }),
                            ))
                            .child(arrow("slide-next", "chevron-right", slide < 2, false).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.slide = (this.slide + 1).min(2);
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        h_flex()
                            .h(px(56.))
                            .items_center()
                            .gap(px(12.))
                            .px(px(16.))
                            .child(
                                ui::mono(11., 16.)
                                    .flex_none()
                                    .text_color(tokens::text3())
                                    .child(format!("{} / 3", slide + 1)),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(1.))
                                    .child(
                                        ui::text(12.5, 17.)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(tokens::text())
                                            .child(title),
                                    )
                                    .child(ui::text(11.5, 16.).truncate().text_color(tokens::text2()).child(sub)),
                            )
                            .child(h_flex().flex_none().gap(px(5.)).children((0..3).map(|i| {
                                div()
                                    .w(px(if i == slide { 16. } else { 6. }))
                                    .h(px(6.))
                                    .rounded(px(3.))
                                    .bg(if i == slide { ui::accent() } else { tokens::border2() })
                            }))),
                    ),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        ui::text(10., 14.)
                            .h(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text3())
                            .child("USE THESE SETTINGS"),
                    )
                    .child(
                        v_flex()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(tokens::border())
                            .child(row("Name", chip("RbxNative").into_any_element(), false))
                            .child(row(
                                "Required",
                                h_flex()
                                    .gap(px(6.))
                                    .child(chip("universe-places:write"))
                                    .child(chip("legacy-asset:manage"))
                                    .child(chip("user.inventory-item:read"))
                                    .into_any_element(),
                                false,
                            ))
                            .child(row(
                                "Optional",
                                h_flex()
                                    .gap(px(6.))
                                    .min_w_0()
                                    .child(div().truncate().text_color(tokens::text2()).child(
                                        "Anything from the list on the next step. Each one switches a feature on.",
                                    ))
                                    .into_any_element(),
                                false,
                            ))
                            .child(row(
                                "Experiences",
                                v_flex()
                                    .gap(px(2.))
                                    .child(
                                        h_flex()
                                            .gap(px(6.))
                                            .child(ui::tag("RECOMMENDED", ui::accent(), tokens::accent_soft()))
                                            .child("Restrict by Experience on, and pick your games."),
                                    )
                                    .child(div().text_color(tokens::text2()).child(
                                        "Every game you pick shows in My Games, private ones too, and the key can\u{2019}t touch anything else.",
                                    ))
                                    .child(div().text_color(tokens::text3()).child(
                                        "Leaving it off reaches every current and future experience, but private ones won\u{2019}t be listed.",
                                    ))
                                    .into_any_element(),
                                false,
                            ))
                            .child(row(
                                "Security",
                                h_flex()
                                    .gap(px(6.))
                                    .child(ui::tag("RECOMMENDED", ui::accent(), tokens::accent_soft()))
                                    .child(div().text_color(tokens::text2()).child(
                                        "Allow only your IP and set an expiration, so a copied key is useless elsewhere.",
                                    ))
                                    .into_any_element(),
                                true,
                            )),
                    ),
            )
            .into_any_element()
    }

    fn paste(&self, cx: &App) -> AnyElement {
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
            .child(key_check::field(&self.check, cx))
            .children(key_check::result(&self.check, Some(table_height), cx))
            .when(ready, |this| {
                this.child(ui::text(11.5, 17.).text_color(tokens::text3()).child(
                    "Optional permissions only switch features on. Add them to the key on the Creator Dashboard any time, then check again.",
                ))
            })
            .children(self.save_error.clone().map(|error| {
                ui::text(11.5, 16.).text_color(ui::red()).child(error)
            }))
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
                    .child(Self::feature_card(
                        "shield-check",
                        "Stored encrypted in your system keychain".into(),
                        "Your IP restriction and expiration are what make a copied key useless, so keep them on.".into(),
                    ))
                    .child(Self::feature_card(
                        "refresh-cw",
                        "Change it any time".into(),
                        "Click your name at the bottom of Home to check the key again, replace it or remove it.".into(),
                    )),
            )
            .into_any_element()
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.check.read(cx).key().is_some();
        let primary: AnyElement = match self.step {
            Step::Welcome => ui::button("wizard-next", "Get started", Weight::Primary, false)
                .min_w(px(112.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.step = Step::Create;
                    cx.notify();
                }))
                .into_any_element(),
            Step::Create => ui::button("wizard-next", "I have my key", Weight::Primary, false)
                .min_w(px(112.))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.step = Step::Paste;
                    let focus = this.check.read(cx).focus.clone();
                    focus.focus(window, cx);
                    cx.notify();
                }))
                .into_any_element(),
            Step::Paste if ready && !self.saving => {
                ui::button("wizard-next", "Continue", Weight::Primary, false)
                    .min_w(px(112.))
                    .on_click(cx.listener(|this, _, _, cx| this.save_and_continue(cx)))
                    .into_any_element()
            }
            Step::Paste => ui::disabled_button("wizard-next", "Continue")
                .min_w(px(112.))
                .into_any_element(),
            Step::Done => ui::button("wizard-next", "Go to Home", Weight::Primary, false)
                .min_w(px(112.))
                .on_click(cx.listener(|this, _, window, cx| this.go_home(window, cx)))
                .into_any_element(),
        };
        h_flex()
            .h(px(64.))
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(32.))
            .border_t_1()
            .border_color(tokens::border())
            .when(self.step != Step::Done, |this| {
                this.child(
                    ui::button("wizard-skip", "Skip for now", Weight::Ghost, false)
                        .on_click(cx.listener(|this, _, window, cx| this.go_home(window, cx))),
                )
            })
            .child(div().flex_1())
            .when(self.step != Step::Welcome, |this| {
                this.child(
                    ui::button("wizard-back", "Back", Weight::Secondary, false)
                        .w(px(96.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.step = match this.step {
                                Step::Done => Step::Paste,
                                Step::Paste => Step::Create,
                                _ => Step::Welcome,
                            };
                            cx.notify();
                        })),
                )
            })
            .child(primary)
    }
}

/// The carousel's Dashboard screenshots (the key in the third is blurred
/// at the source), decoded once.
fn slide_image(index: usize) -> Option<Arc<RenderImage>> {
    static SLIDES: OnceLock<Vec<Option<Arc<RenderImage>>>> = OnceLock::new();
    const PNGS: [&[u8]; 3] = [
        include_bytes!("../../../../assets/launcher/dashboard-create-key.png"),
        include_bytes!("../../../../assets/launcher/dashboard-permissions.png"),
        include_bytes!("../../../../assets/launcher/dashboard-copy-key.png"),
    ];
    SLIDES
        .get_or_init(|| {
            PNGS.iter()
                .map(|bytes| {
                    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    crate::render_image::to_render_image(rgba.into_raw(), w, h)
                })
                .collect()
        })
        .get(index)
        .cloned()
        .flatten()
}

impl Render for Wizard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.step {
            Step::Welcome => self.welcome(),
            Step::Create => self.create(cx),
            Step::Paste => self.paste(cx),
            Step::Done => self.done(cx),
        };
        v_flex()
            .size_full()
            .bg(ui::panel())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_color(tokens::text())
            .text_size(px(13.))
            .child(ui::titlebar(
                "Set up publishing",
                self.grab.clone(),
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
