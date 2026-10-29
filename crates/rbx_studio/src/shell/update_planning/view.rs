//! The window's frame: title bar, the experiences sidebar, the selected
//! experience's header and banner, then the planner or the steps view.

use std::sync::Arc;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui;
use crate::tokens;
use crate::update_planning::plan::{countdown, Plan};
use crate::update_planning::when;

use super::UpdatePlanningWindow;

mod planner;
mod steps;

const SIDEBAR: f32 = 248.;

impl Render for UpdatePlanningWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = chrono::Utc::now().timestamp();
        let body = match self.plan() {
            Some(plan) if !self.replan => self.steps_view(plan, now, cx).into_any_element(),
            _ if self.selected.is_some() => self.planner(now, cx).into_any_element(),
            _ => empty_state().into_any_element(),
        };
        v_flex()
            .id("update-planning-window")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .text_size(px(13.))
            .text_color(tokens::text())
            .child(crate::shell::chrome::window_topbar(
                "Update Planning".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(now, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .children(self.header(now))
                            .children(self.banner())
                            .child(div().flex_1().min_h_0().child(body)),
                    ),
            )
    }
}

fn empty_state() -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .child(ui::dialog_glyph(
            "calendar-clock",
            ui::accent(),
            tokens::accent_soft(),
        ))
        .child(
            div()
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("No experiences yet"),
        )
        .child(
            div()
                .text_color(tokens::text3())
                .child("Open Home once so it lists your games, or link this place to one."),
        )
}

/// One line on what a plan is doing, for the sidebar.
fn plan_line(plan: Option<&Plan>, now: i64) -> (String, Rgba) {
    match plan {
        None => ("No update planned".into(), tokens::text3()),
        Some(plan) if plan.cancelled => ("Cancelled".into(), tokens::text3()),
        Some(plan) if plan.finished() => (
            format!("Updated {}", when::label(plan.update_at)),
            ui::green(),
        ),
        Some(plan) => (
            format!("Update in {}", countdown(plan.update_at - now)),
            ui::accent(),
        ),
    }
}

/// A rounded square image, or a placeholder glyph.
pub(super) fn picture(image: Option<Arc<RenderImage>>, side: f32, glyph: &'static str) -> Div {
    div()
        .size(px(side))
        .flex_none()
        .rounded(tokens::radius_container())
        .overflow_hidden()
        .border_1()
        .border_color(tokens::border())
        .bg(tokens::field_select())
        .map(|d| match image {
            Some(image) => d.child(img(image).size_full().object_fit(ObjectFit::Cover)),
            None => d
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens::text3())
                .child(ui::icon(glyph, (side / 3.).min(28.))),
        })
}

/// A labelled single-line field.
pub(super) fn field(input: &Entity<InputState>, glyph: &'static str, width: f32) -> Div {
    h_flex()
        .w(px(width))
        .h(tokens::input_height())
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .rounded(tokens::radius())
        .border_1()
        .border_color(tokens::border())
        .bg(tokens::field_select())
        .child(
            div()
                .text_color(tokens::text3())
                .child(ui::icon(glyph, 13.)),
        )
        .child(
            Input::new(input)
                .appearance(false)
                .with_size(tokens::field_size())
                .h(tokens::input_height())
                .flex_1(),
        )
}

/// A titled card, the planner's and steps view's one container.
pub(super) fn card(title: &'static str, hint: &'static str) -> Div {
    v_flex()
        .w_full()
        .p(tokens::panel_padding() * 1.5)
        .gap(tokens::section_gap())
        .rounded(tokens::radius_container())
        .border_1()
        .border_color(tokens::border())
        .bg(ui::panel())
        .child(
            v_flex()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(tokens::text3())
                        .child(hint),
                ),
        )
}

impl UpdatePlanningWindow {
    fn sidebar(&self, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("up-sidebar")
            .w(px(SIDEBAR))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .p(px(8.))
            .gap(px(2.))
            .bg(tokens::chrome())
            .border_r_1()
            .border_color(tokens::border())
            .child(
                div()
                    .px(px(8.))
                    .pt(px(6.))
                    .pb(px(8.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens::text3())
                    .child("EXPERIENCES"),
            )
            .children(self.experiences.iter().map(|game| {
                let id = game.universe_id;
                let selected = self.selected == Some(id);
                let (line, color) = plan_line(self.plans.get(&id), now);
                h_flex()
                    .id(SharedString::from(format!("up-game-{id}")))
                    .h(px(52.))
                    .px(px(8.))
                    .gap(px(10.))
                    .items_center()
                    .rounded(tokens::radius())
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(tokens::selection()).text_color(tokens::text_full())
                    })
                    .when(!selected, |d| d.hover(|h| h.bg(tokens::hover())))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(id, cx)))
                    .child(picture(self.thumbs.get(&id).cloned(), 36., "image"))
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap(px(1.))
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(game.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(if selected { tokens::text_full() } else { color })
                                    .when(selected, |d| d.opacity(0.85))
                                    .child(line),
                            ),
                    )
            }))
    }

    fn header(&self, now: i64) -> Option<impl IntoElement> {
        let game = self.experience()?;
        let running = self.plan().filter(|p| !p.finished());
        let next = running.and_then(|p| p.next_at(now));
        Some(
            h_flex()
                .flex_none()
                .px(px(24.))
                .py(px(18.))
                .gap(px(14.))
                .items_center()
                .border_b_1()
                .border_color(tokens::border())
                .child(picture(
                    self.thumbs.get(&game.universe_id).cloned(),
                    52.,
                    "image",
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(3.))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(game.name.clone()),
                        )
                        .child(div().text_size(px(12.)).text_color(tokens::text3()).child(
                            format!(
                                "Universe {} \u{00b7} icon language {}",
                                game.universe_id, self.language
                            ),
                        )),
                )
                .when_some(running, |d, plan| {
                    d.child(
                        v_flex()
                            .items_end()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(tokens::text3())
                                    .child("UPDATE IN"),
                            )
                            .child(
                                div()
                                    .font_family(tokens::font_mono())
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(ui::accent())
                                    .child(countdown(plan.update_at - now)),
                            )
                            .when_some(next.filter(|&n| n < plan.update_at), |d, n| {
                                d.child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(tokens::text3())
                                        .child(format!("next step in {}", countdown(n - now))),
                                )
                            }),
                    )
                }),
        )
    }

    fn banner(&self) -> Option<impl IntoElement> {
        let busy = self.busy.clone();
        let status = self.status.clone();
        if busy.is_none() && status.is_none() {
            return None;
        }
        let (error, text) = match (&busy, status) {
            (Some(busy), _) => (false, busy.clone()),
            (None, Some(status)) => status,
            (None, None) => return None,
        };
        Some(
            h_flex()
                .flex_none()
                .mx(px(24.))
                .mt(px(14.))
                .px(px(12.))
                .py(px(8.))
                .gap(px(8.))
                .items_center()
                .rounded(tokens::radius())
                .bg(if error {
                    ui::red_soft()
                } else {
                    tokens::accent_soft()
                })
                .text_color(if error { ui::red() } else { tokens::text() })
                .text_size(px(12.5))
                .map(|d| match busy {
                    Some(_) => d.child(ui::spinner("up-busy", 13.)),
                    None => d.child(ui::icon(if error { "circle-alert" } else { "info" }, 13.)),
                })
                .child(div().flex_1().child(text)),
        )
    }
}
