//! The wizard's rail (steps and the why-a-key note) and its footer.

use super::ui::{self, Weight};
use crate::tokens;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;

use super::*;

impl Wizard {
    pub(super) fn rail(&self) -> impl IntoElement {
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

    pub(super) fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
