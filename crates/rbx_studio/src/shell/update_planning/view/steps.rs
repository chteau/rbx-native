//! A started update: every step with its review and its state, the
//! progress, and Cancel (or, once it is over, a way to plan the next).

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::tokens;
use crate::update_planning::plan::{countdown, Plan, Review, StepState};
use crate::update_planning::when;

use super::super::UpdatePlanningWindow;
use super::{card, picture};

fn review_tag(review: Review) -> Div {
    match review {
        Review::Pending => ui::tag("UPLOADING", tokens::text2(), ui::wash()),
        Review::Uploaded => ui::tag("IN REVIEW", tokens::warning(), tokens::error_soft()),
        Review::Approved => ui::tag("APPROVED", ui::green(), ui::green_soft()),
        Review::Rejected => ui::tag("REJECTED", ui::red(), ui::red_soft()),
    }
}

fn state_tag(state: StepState) -> Div {
    match state {
        StepState::Upcoming => ui::tag("UPCOMING", ui::accent(), tokens::accent_soft()),
        StepState::Due => ui::tag("APPLYING", tokens::warning(), tokens::error_soft()),
        StepState::Applied => ui::tag("APPLIED", ui::green(), ui::green_soft()),
        StepState::Skipped => ui::tag("SKIPPED", tokens::text3(), ui::wash()),
        StepState::Cancelled => ui::tag("CANCELLED", tokens::text3(), ui::wash()),
    }
}

impl UpdatePlanningWindow {
    pub(super) fn steps_view(
        &self,
        plan: &Plan,
        now: i64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let applied = plan.steps.iter().filter(|s| s.applied_at.is_some()).count();
        let approved = plan
            .steps
            .iter()
            .filter(|s| s.review == Review::Approved)
            .count();
        let total = plan.steps.len();
        let hint = if plan.cancelled {
            "Cancelled. Plan the next one whenever you like."
        } else if plan.finished() {
            "Done: the new icon is live. Plan the next one whenever you like."
        } else {
            "Each image was uploaded at the start so Roblox reviews it early. Steps fire while the editor is open, and from the background helper while the PC is on."
        };
        v_flex()
            .id("up-steps")
            .size_full()
            .overflow_y_scroll()
            .px(px(24.))
            .py(px(18.))
            .gap(tokens::group_gap())
            .child(
                card("Steps", hint)
                    .child(
                        h_flex()
                            .gap(px(18.))
                            .items_center()
                            .child(stat("Update", when::label(plan.update_at)))
                            .child(stat("Applied", format!("{applied} of {total}")))
                            .child(stat("Approved", format!("{approved} of {total}")))
                            .child(div().flex_1())
                            .map(|d| {
                                if plan.finished() {
                                    d.child(
                                        ui::icon_button(
                                            "up-replan",
                                            "plus",
                                            "Plan a new update",
                                            Weight::Primary,
                                            false,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.replan = true;
                                                this.fetch_current(cx);
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                } else {
                                    d.child(
                                        ui::icon_button(
                                            "up-cancel",
                                            "ban",
                                            "Cancel update",
                                            Weight::Danger,
                                            false,
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                                    )
                                }
                            }),
                    )
                    .child(
                        v_flex()
                            .gap(px(2.))
                            .children(plan.steps.iter().enumerate().map(|(i, step)| {
                                let state = plan.state(i, now);
                                let image = self
                                    .step_images
                                    .get(&(plan.universe_id, step.image.clone()))
                                    .cloned();
                                let time = match state {
                                    StepState::Upcoming => format!(
                                        "{} \u{00b7} in {}",
                                        when::label(step.at),
                                        countdown(step.at - now)
                                    ),
                                    StepState::Applied => format!(
                                        "Applied {}",
                                        when::label(step.applied_at.unwrap_or(step.at))
                                    ),
                                    _ => when::label(step.at),
                                };
                                h_flex()
                                    .id(SharedString::from(format!("up-step-{i}")))
                                    .px(px(8.))
                                    .py(px(8.))
                                    .gap(px(14.))
                                    .items_center()
                                    .rounded(tokens::radius())
                                    .when(
                                        state == StepState::Due
                                            || (state == StepState::Upcoming
                                                && plan.next_at(now) == Some(step.at)),
                                        |d| d.bg(tokens::hover_subtle()),
                                    )
                                    .child(picture(image, 64., "image"))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(step.label.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(tokens::text3())
                                                    .child(time),
                                            )
                                            .when_some(step.error.clone(), |d, err| {
                                                d.child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(ui::red())
                                                        .child(format!("Last try failed: {err}")),
                                                )
                                            }),
                                    )
                                    .child(review_tag(step.review))
                                    .child(
                                        div()
                                            .w(px(84.))
                                            .flex()
                                            .justify_end()
                                            .child(state_tag(state)),
                                    )
                            })),
                    ),
            )
            .when(!plan.finished(), |d| {
                d.child(
                    card(
                        "While the editor is closed",
                        "Steps keep firing as long as the PC is on and you are logged in.",
                    )
                    .child(info_row("clock", crate::update_planning::helper::describe()))
                    .child(info_row(
                        "refresh-cw",
                        "A run that comes late applies the icon due now and skips the ones it missed.",
                    ))
                    .child(info_row(
                        "bell",
                        "Each step that goes live sends a desktop notification.",
                    )),
                )
            })
    }
}

fn info_row(glyph: &'static str, text: &'static str) -> impl IntoElement {
    h_flex()
        .gap(px(10.))
        .text_size(px(12.5))
        .text_color(tokens::text2())
        .child(
            div()
                .text_color(tokens::text3())
                .child(ui::icon(glyph, 14.)),
        )
        .child(text)
}

fn stat(label: &'static str, value: String) -> impl IntoElement {
    v_flex()
        .gap(px(1.))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(tokens::text3())
                .child(label),
        )
        .child(div().font_weight(FontWeight::SEMIBOLD).child(value))
}
