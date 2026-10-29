//! Planning an update: the two icons, the update time, the timers and the
//! timeline of what will show when, then Start update.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::tokens;
use crate::update_planning::overlay::Overlay;
use crate::update_planning::plan::countdown;
use crate::update_planning::when;

use super::super::work::Pick;
use super::super::{Picked, UpdatePlanningWindow};
use super::{card, field, picture};

const SLOT: f32 = 148.;
const CARD: f32 = 128.;

/// "6 h before", "90 min before", "1 h 30 min before".
fn lead(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min before"),
        (h, 0) => format!("{h} h before"),
        (h, m) => format!("{h} h {m} min before"),
    }
}

impl UpdatePlanningWindow {
    pub(super) fn planner(&self, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .id("up-planner")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .py(px(18.))
                    .gap(tokens::group_gap())
                    .child(
                        h_flex()
                            .w_full()
                            .items_stretch()
                            .gap(tokens::group_gap())
                            .child(self.icons_card(cx))
                            .child(self.time_card(now, cx)),
                    )
                    .child(self.timers_card(cx))
                    .child(self.timeline_card(now, cx)),
            )
            .child(self.start_bar(cx))
    }

    fn icon_slot(
        &self,
        id: &'static str,
        title: &'static str,
        picked: Option<&Picked>,
        target: Pick,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap(px(8.))
            .child(picture(
                picked.map(|p| p.preview.clone()),
                SLOT,
                "image-plus",
            ))
            .child(
                v_flex()
                    .gap(px(1.))
                    .child(div().font_weight(FontWeight::MEDIUM).child(title))
                    .child(
                        div()
                            .w(px(SLOT))
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(tokens::text3())
                            .child(
                                picked.map_or("Not picked yet".to_string(), |p| p.source.clone()),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .when(target == Pick::Current, |d| {
                        d.child(
                            ui::icon_button(
                                SharedString::from(format!("{id}-fetch")),
                                "refresh-cw",
                                "Roblox",
                                Weight::Secondary,
                                true,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.fetch_current(cx))),
                        )
                    })
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("{id}-file")),
                            "folder-open",
                            "File\u{2026}",
                            Weight::Secondary,
                            true,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.pick(target, cx))),
                    ),
            )
    }

    fn icons_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        card(
            "Icons",
            "The timers go over the current icon; the new one replaces it at the update.",
        )
        .flex_1()
        .child(
            h_flex()
                .gap(px(18.))
                .items_start()
                .child(self.icon_slot(
                    "up-current",
                    "Current icon",
                    self.current.as_ref(),
                    Pick::Current,
                    cx,
                ))
                .child(
                    div()
                        .h(px(SLOT))
                        .flex()
                        .items_center()
                        .text_color(tokens::text3())
                        .child(ui::icon("arrow-right", 18.)),
                )
                .child(self.icon_slot("up-new", "New icon", self.new_icon.as_ref(), Pick::New, cx)),
        )
    }

    fn time_card(&self, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let parsed = self.update_at(cx);
        card(
            "Update time",
            "Your local time. The new icon goes live then.",
        )
        .w(px(340.))
        .flex_none()
        .child(field(&self.when, "calendar", 280.))
        .child(match &parsed {
            Ok(at) => h_flex()
                .gap(px(6.))
                .text_size(px(12.5))
                .child(ui::icon("calendar-clock", 13.))
                .child(format!(
                    "{} \u{00b7} in {}",
                    when::label(*at),
                    countdown(at - now)
                )),
            Err(err) => h_flex()
                .gap(px(6.))
                .text_size(px(12.5))
                .text_color(ui::red())
                .child(ui::icon("circle-alert", 13.))
                .child(err.clone()),
        })
        .child(div().flex_1())
        .child(self.summary(parsed.ok(), now))
    }

    /// What Start would set going, under the time.
    fn summary(&self, update_at: Option<i64>, now: i64) -> impl IntoElement {
        let first = self
            .previews
            .iter()
            .filter_map(|p| p.minutes_before)
            .max()
            .zip(update_at)
            .map(|(m, at)| at - i64::from(m) * 60);
        let rows = [
            ("Steps", self.previews.len().to_string()),
            (
                "First change",
                match first {
                    Some(at) if at <= now => "Now, at Start".to_string(),
                    Some(at) => when::label(at),
                    None => "\u{2014}".to_string(),
                },
            ),
            ("Icon language", self.language.clone()),
            ("Uploads", "At Start, for review".to_string()),
        ];
        v_flex()
            .pt(tokens::section_gap())
            .gap(px(6.))
            .border_t_1()
            .border_color(tokens::border())
            .children(rows.into_iter().map(|(label, value)| {
                h_flex()
                    .text_size(px(12.5))
                    .child(div().w(px(110.)).text_color(tokens::text3()).child(label))
                    .child(div().font_weight(FontWeight::MEDIUM).child(value))
            }))
    }

    fn timers_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        card(
            "Timers",
            "Reused by every update of this experience. Edits apply to the next update you start, never to one already running.",
        )
        .child(
            v_flex()
                .gap(px(2.))
                .children(self.timers.iter().enumerate().map(|(i, timer)| {
                    let style = match &timer.overlay {
                        Overlay::Builtin(style) => style.name(),
                        Overlay::File(_) => "Custom image",
                    };
                    h_flex()
                        .id(SharedString::from(format!("up-timer-{i}")))
                        .h(px(52.))
                        .px(px(8.))
                        .gap(px(12.))
                        .items_center()
                        .rounded(tokens::radius())
                        .hover(|h| h.bg(tokens::hover_subtle()))
                        .child(picture(self.timer_thumb(i), 40., "image").bg(tokens::black()))
                        .child(div().w(px(150.)).truncate().font_weight(FontWeight::MEDIUM).child(timer.label.clone()))
                        .child(div().flex_1().text_color(tokens::text2()).child(lead(timer.minutes_before)))
                        .child(
                            ui::icon_button(SharedString::from(format!("up-style-{i}")), "palette", style, Weight::Ghost, true)
                                .on_click(cx.listener(move |this, _, _, cx| this.cycle_style(i, cx))),
                        )
                        .child(
                            ui::icon_button(SharedString::from(format!("up-custom-{i}")), "image-up", "Own image\u{2026}", Weight::Ghost, true)
                                .on_click(cx.listener(move |this, _, _, cx| this.pick(Pick::Overlay(i), cx))),
                        )
                        .child(
                            ui::icon_button(SharedString::from(format!("up-remove-{i}")), "x", "", Weight::Ghost, true)
                                .on_click(cx.listener(move |this, _, _, cx| this.remove_timer(i, cx))),
                        )
                })),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .pt(px(4.))
                .border_t_1()
                .border_color(tokens::border())
                .pt(tokens::section_gap())
                .child(field(&self.timer_label, "tag", 220.))
                .child(field(&self.timer_hours, "hourglass", 150.))
                .child(
                    ui::icon_button("up-add-timer", "plus", "Add timer", Weight::Secondary, false)
                        .on_click(cx.listener(|this, _, window, cx| this.add_timer(window, cx))),
                ),
        )
    }

    fn timeline_card(&self, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let update_at = self.update_at(cx).ok();
        card(
            "Timeline",
            "What the experience's icon will be at each step.",
        )
        .child(
            h_flex()
                .flex_wrap()
                .gap(px(14.))
                .when(self.previews.is_empty(), |d| {
                    d.child(
                        div()
                            .text_color(tokens::text3())
                            .child("Pick the current icon to see the timers on it."),
                    )
                })
                .children(self.previews.iter().map(|preview| {
                    let at =
                        update_at.map(|t| t - i64::from(preview.minutes_before.unwrap_or(0)) * 60);
                    let past = at.is_some_and(|at| at <= now) && preview.minutes_before.is_some();
                    let time = match (at, preview.minutes_before) {
                        (Some(at), _) => when::label(at),
                        (None, Some(m)) => lead(m),
                        (None, None) => "At the update".to_string(),
                    };
                    v_flex()
                        .w(px(CARD))
                        .gap(px(6.))
                        .when(past, |d| d.opacity(0.45))
                        .child(picture(Some(preview.image.clone()), CARD, "image"))
                        .child(
                            v_flex()
                                .child(
                                    div()
                                        .truncate()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(preview.label.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(11.5))
                                        .text_color(tokens::text3())
                                        .child(if past {
                                            "Already passed".to_string()
                                        } else {
                                            time
                                        }),
                                ),
                        )
                })),
        )
    }

    fn start_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let blocker = self.start_blocker(cx);
        let count = self.previews.len();
        h_flex()
            .flex_none()
            .h(px(64.))
            .px(px(24.))
            .gap(px(12.))
            .items_center()
            .border_t_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.5))
                    .text_color(tokens::text2())
                    .child(blocker.clone().unwrap_or_else(|| {
                        format!("Uploads {count} images now so Roblox reviews them early; each becomes the icon at its time.")
                    })),
            )
            .map(|d| match blocker {
                Some(_) => d.child(ui::disabled_button("up-start", "Start update")),
                None => d.child(
                    ui::icon_button("up-start", "rocket", "Start update", Weight::Primary, false)
                        .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
                ),
            })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn leads_read_naturally() {
        assert_eq!(super::lead(720), "12 h before");
        assert_eq!(super::lead(90), "1 h 30 min before");
        assert_eq!(super::lead(30), "30 min before");
    }
}
