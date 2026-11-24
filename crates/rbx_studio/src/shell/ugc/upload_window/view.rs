//! The Upload Avatar Item window's layout: four numbered cards and a footer.

use std::time::Duration;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_ugc::Severity;

use super::super::check::Run;
use super::super::upload::{creator_hub_url, Creator};
use super::super::{pickers, report, ItemSource};
use super::{fees, Phase, UploadWindow};
use crate::launcher::ui::{self, Weight};
use crate::tokens;

fn card(number: usize, title: &'static str, hint: &'static str, body: impl IntoElement) -> Div {
    v_flex()
        .gap(px(12.))
        .p(px(16.))
        .rounded(px(8.))
        .bg(ui::panel())
        .border_1()
        .border_color(tokens::border())
        .child(
            h_flex()
                .gap(px(10.))
                .child(
                    div()
                        .flex_none()
                        .size(px(22.))
                        .rounded_full()
                        .bg(ui::accent())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            ui::text(12., 16.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(tokens::text_full())
                                .child(number.to_string()),
                        ),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .child(
                            ui::text(14., 20.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(tokens::text_strong())
                                .child(title),
                        )
                        .child(
                            ui::text(12., 16.)
                                .text_color(tokens::text2())
                                .truncate()
                                .child(hint),
                        ),
                ),
        )
        .child(body)
}

fn field(label: &'static str, glyph: &'static str, state: &Entity<InputState>, busy: bool) -> Div {
    v_flex()
        .gap(px(4.))
        .child(ui::text(12., 16.).text_color(tokens::text2()).child(label))
        .child(
            ui::field_frame(None, ui::panel2(), tokens::border(), glyph).child(
                div().flex_1().min_w_0().h_full().child(
                    Input::new(state)
                        .appearance(false)
                        .h_full()
                        .px(px(0.))
                        .text_size(px(12.))
                        .text_color(tokens::text())
                        .disabled(busy),
                ),
            ),
        )
}

fn creator_pill(id: SharedString, label: String, glyph: &'static str, on: bool) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(28.))
        .px(px(10.))
        .gap(px(6.))
        .rounded(px(6.))
        .border_1()
        .cursor_pointer()
        .when(on, |el| {
            el.bg(tokens::accent_soft())
                .border_color(tokens::accent_line())
                .text_color(ui::accent())
        })
        .when(!on, |el| {
            el.bg(ui::panel2())
                .border_color(tokens::border())
                .text_color(tokens::text())
                .hover(|s| s.bg(tokens::hover()))
        })
        .child(ui::icon(glyph, 14.))
        .child(ui::text(12., 16.).child(label))
}

fn status(glyph: Option<&'static str>, text: String, color: Rgba, spin: bool) -> Div {
    h_flex()
        .gap(px(8.))
        .min_w_0()
        .text_color(color)
        .when(spin, |el| el.child(ui::spinner("ugc-upload-spin", 14.)))
        .when_some(glyph, |el, g| el.child(ui::icon(g, 14.)))
        .child(ui::text(12.5, 18.).truncate().child(text))
}

impl UploadWindow {
    fn details(&self, cx: &mut Context<Self>) -> Div {
        let busy = matches!(self.phase, Phase::Sending);
        let mut pills = h_flex().flex_wrap().gap(px(6.)).child(
            creator_pill(
                "ugc-upload-me".into(),
                "Your account".into(),
                "user",
                self.creator == Creator::Me,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.creator = Creator::Me;
                cx.notify();
            })),
        );
        for group in &self.groups {
            let id = group.id;
            pills = pills.child(
                creator_pill(
                    SharedString::from(format!("ugc-upload-group-{id}")),
                    group.name.clone(),
                    "users",
                    self.creator == Creator::Group(id),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.creator = Creator::Group(id);
                    cx.notify();
                })),
            );
        }
        v_flex()
            .gap(px(12.))
            .child(field("Title", "type", &self.title, busy))
            .child(field(
                "Description",
                "text-align-start",
                &self.description,
                busy,
            ))
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        ui::text(12., 16.)
                            .text_color(tokens::text2())
                            .child("Creator"),
                    )
                    .child(pills)
                    .when(self.groups.is_empty(), |el| {
                        el.child(
                            ui::text(12., 16.)
                                .text_color(tokens::text2())
                                .child("Your groups appear here once Home has listed them."),
                        )
                    }),
            )
    }

    fn check_body(&self) -> AnyElement {
        let subject = self
            .check
            .source
            .as_ref()
            .map(ItemSource::label)
            .unwrap_or_default();
        match &self.check.run {
            Run::Idle => ui::text(12.5, 18.)
                .text_color(tokens::text2())
                .child("Pick an item above to check it.")
                .into_any_element(),
            Run::Running => {
                status(None, "Checking\u{2026}".into(), tokens::text2(), true).into_any_element()
            }
            Run::Failed(msg) => {
                status(Some("circle-x"), msg.clone(), ui::red(), false).into_any_element()
            }
            Run::Done(done) => v_flex()
                .gap(px(12.))
                .child(
                    ui::text(13., 18.)
                        .text_color(tokens::text_strong())
                        .child(report::summary(&done.findings)),
                )
                .child(report::findings_view(
                    "ugc-upload",
                    &done.findings,
                    &subject,
                    None,
                ))
                .into_any_element(),
        }
    }

    fn status(&self) -> Div {
        if let Phase::Sending = self.phase {
            return status(None, "Uploading\u{2026}".into(), tokens::text2(), true);
        }
        if let Phase::Failed(msg) = &self.phase {
            return status(Some("circle-x"), msg.clone(), ui::red(), false);
        }
        if let Phase::Done(id) = self.phase {
            return status(
                Some("circle-check"),
                format!("Uploaded as Model {id}"),
                ui::green(),
                false,
            );
        }
        if self.check.source.is_none() {
            return status(None, "Pick an item first".into(), tokens::text2(), false);
        }
        match &self.check.run {
            Run::Running => status(None, "Checking\u{2026}".into(), tokens::text2(), true),
            Run::Done(done) if self.check.blocking() => {
                let n = done
                    .findings
                    .iter()
                    .filter(|f| f.severity == Severity::Error)
                    .count();
                let noun = if n == 1 { "error" } else { "errors" };
                status(
                    Some("circle-x"),
                    format!("Fix the {n} {noun} above to upload"),
                    ui::red(),
                    false,
                )
            }
            Run::Failed(msg) => status(Some("circle-x"), msg.clone(), ui::red(), false),
            _ => div(),
        }
    }

    fn done_panel(&self, id: u64) -> AnyElement {
        let panel = h_flex()
            .gap(px(10.))
            .mx(px(20.))
            .mb(px(8.))
            .p(px(12.))
            .rounded(px(8.))
            .bg(ui::green_soft())
            .child(ui::icon("badge-check", 18.).text_color(ui::green()))
            .child(
                ui::text(12.5, 18.)
                    .text_color(tokens::text())
                    .child(format!(
                    "Model {id} is on Roblox. Finish it on the Creator Hub page that just opened."
                )),
            );
        if tokens::reduced_motion() {
            return panel.into_any_element();
        }
        panel
            .with_animation(
                SharedString::from(format!("ugc-upload-done-{id}")),
                Animation::new(Duration::from_millis(220)).with_easing(tokens::easing_soft),
                |el, t| el.opacity(t).mt(px(8. * (1. - t))),
            )
            .into_any_element()
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let close = ui::button("ugc-upload-close", "Close", Weight::Secondary, false)
            .on_click(|_, window, cx| window.defer(cx, |window, _| window.remove_window()));
        let actions = h_flex().flex_none().gap(px(8.)).child(close);
        let actions = if let Phase::Done(id) = self.phase {
            actions
                .child(
                    ui::icon_button(
                        "ugc-upload-copy",
                        "copy",
                        "Copy ID",
                        Weight::Secondary,
                        false,
                    )
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(id.to_string()))
                    }),
                )
                .child(
                    ui::external_button(
                        "ugc-upload-hub",
                        "Open on Creator Hub",
                        Weight::Primary,
                        false,
                    )
                    .on_click(move |_, _, cx| cx.open_url(&creator_hub_url(id))),
                )
        } else if self.can_upload(cx) {
            actions.child(
                ui::icon_button(
                    "ugc-upload-go",
                    "cloud-upload",
                    "Upload Model",
                    Weight::Primary,
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| this.upload(cx))),
            )
        } else {
            actions.child(ui::disabled_button("ugc-upload-go", "Upload Model"))
        };
        h_flex()
            .flex_none()
            .justify_between()
            .gap(px(12.))
            .px(px(20.))
            .py(px(12.))
            .border_t_1()
            .border_color(tokens::border())
            .bg(ui::panel())
            .child(div().flex_1().min_w_0().child(self.status()))
            .child(actions)
    }
}

impl Render for UploadWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let on_selection = {
            let this = this.clone();
            move |window: &mut Window, cx: &mut App| {
                this.update(cx, |v, cx| v.use_selection(window, cx))
            }
        };
        let on_file = {
            let this = this.clone();
            move |window: &mut Window, cx: &mut App| {
                this.update(cx, |v, cx| v.choose_file(window, cx))
            }
        };
        let on_pick = {
            let this = this.clone();
            move |kind, _: &mut Window, cx: &mut App| this.update(cx, |v, cx| v.set_kind(kind, cx))
        };
        let busy = matches!(self.phase, Phase::Sending);
        let emissive = match &self.check.run {
            Run::Done(done) => Some(done.collected.emissive),
            _ => None,
        };
        let item = v_flex()
            .gap(px(16.))
            .child(pickers::source_card(
                "ugc-upload-source",
                self.check.source.as_ref(),
                on_selection,
                on_file,
            ))
            .child(pickers::kind_picker(
                "ugc-upload-kind",
                self.check.kind,
                busy,
                on_pick,
            ));
        let done = match self.phase {
            Phase::Done(id) => Some(self.done_panel(id)),
            _ => None,
        };
        v_flex()
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .child(crate::shell::chrome::window_topbar(
                "Upload Avatar Item".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(
                div()
                    .id("ugc-upload-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        v_flex()
                            .gap(px(16.))
                            .p(px(20.))
                            .child(card(
                                1,
                                "Item",
                                "What to upload, and the avatar item type it is.",
                                item,
                            ))
                            .child(card(
                                2,
                                "Details",
                                "The name, description and owner of the new Model.",
                                self.details(cx),
                            ))
                            .child(card(
                                3,
                                "Check",
                                "The documented Marketplace limits, checked here first.",
                                self.check_body(),
                            ))
                            .child(card(
                                4,
                                "Fees",
                                "What uploading costs, here and on the Creator Hub.",
                                fees::fees_view(self.check.kind, emissive),
                            )),
                    ),
            )
            .children(done)
            .child(self.footer(cx))
    }
}
