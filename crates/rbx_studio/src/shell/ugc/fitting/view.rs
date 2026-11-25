//! The window's frame: title bar, step rail, the step's cards, the live
//! check, and the footer that moves between steps.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::check::Run;
use super::super::report;
use super::state::Step;
use super::widgets::{card, entrance, note, rail, GAP, PAD};
use super::{FittingWindow, Job, TITLE};
use crate::launcher::ui::{self, Weight};
use crate::tokens;

impl FittingWindow {
    /// The live check of the working item: a summary line, then the
    /// findings.
    fn check_card(&self) -> Div {
        let subject = self
            .generated
            .as_ref()
            .map(|g| g.name.clone())
            .or_else(|| self.source.as_ref().map(|s| s.name.clone()))
            .unwrap_or_else(|| "The item".into());
        let (hint, body): (String, AnyElement) = match &self.check.run {
            Run::Done(done) => (
                report::summary(&done.findings),
                report::findings_view("ugc-fit-report", &done.findings, &subject, None),
            ),
            Run::Running => (
                "Checking\u{2026}".into(),
                h_flex()
                    .gap(px(8.))
                    .child(ui::spinner("ugc-fit-check-spin", 14.))
                    .child(
                        ui::text(12., 16.)
                            .text_color(tokens::text2())
                            .child("Reading the item\u{2026}"),
                    )
                    .into_any_element(),
            ),
            Run::Failed(msg) => (
                "The check could not run".into(),
                note("circle-x", tokens::text_error(), msg.clone()).into_any_element(),
            ),
            Run::Idle => (
                "Runs on every change".into(),
                note(
                    "info",
                    tokens::text3(),
                    "Pick an item and the check runs on it as you fit it.",
                )
                .into_any_element(),
            ),
        };
        card("shield-check", "Check", hint, body)
    }

    fn step_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Div> {
        match self.step {
            Step::Item => self.item_cards(cx),
            Step::Fit => self.fit_cards(cx),
            Step::Test => self.test_cards(window, cx),
            Step::Generate => self.generate_cards(),
        }
    }

    fn generate_cards(&self) -> Vec<Div> {
        let name = self.source.as_ref().map(|s| s.name.clone());
        let body = match &self.generated {
            None => {
                let what = name.unwrap_or_else(|| "the item".into());
                let mut lines = v_flex().gap(px(8.)).child(note(
                    "package",
                    tokens::text2(),
                    format!(
                        "Writes {what} into Workspace as an Accessory, where it sits on the fitting body now, and selects it. Undo takes it back."
                    ),
                ));
                if self.settings.layered() && self.cages.ready().is_none() {
                    lines = lines.child(note(
                        "triangle-alert",
                        tokens::warning(),
                        "The cages aren\u{2019}t made yet. Open the Fit step to make them.",
                    ));
                }
                lines
            }
            Some(g) => {
                let upload = match &g.upload {
                    Job::Ready(super::super::ItemSource::File(path)) => note(
                        "file-box",
                        tokens::text2(),
                        format!(
                            "Upload Item sends {} (the mesh and both cages): Roblox can\u{2019}t read meshes that live only in this editor.",
                            path.display()
                        ),
                    ),
                    Job::Ready(_) => note(
                        "cloud-upload",
                        tokens::text2(),
                        "Upload Item sends the accessory itself: all of its meshes are already on Roblox.",
                    ),
                    Job::Busy => note("loader-circle", tokens::text2(), "Writing the upload file\u{2026}"),
                    Job::Failed(e) => note(
                        "circle-x",
                        tokens::text_error(),
                        format!("Upload Item can\u{2019}t send this item: {e}"),
                    ),
                    Job::Idle => note("info", tokens::text3(), ""),
                };
                v_flex()
                    .gap(px(8.))
                    .child(note(
                        "badge-check",
                        ui::green(),
                        format!("{} is in Workspace and selected.", g.name),
                    ))
                    .child(upload)
            }
        };
        vec![card(
            "package",
            "Generate",
            "The accessory Studio\u{2019}s fitting tool would write",
            body,
        )]
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let index = self.step.index();
        let back = (index > 0).then(|| {
            ui::icon_button(
                "ugc-fit-back",
                "arrow-left",
                "Back",
                Weight::Secondary,
                false,
            )
            .on_click(cx.listener(move |v, _, _, cx| v.go(Step::ALL[index - 1], cx)))
        });
        let primary =
            match self.step {
                Step::Generate => match &self.generated {
                    None if self.can_generate() => ui::icon_button(
                        "ugc-fit-generate",
                        "sparkles",
                        "Generate Accessory",
                        Weight::Primary,
                        false,
                    )
                    .on_click(cx.listener(|v, _, _, cx| v.generate(cx)))
                    .into_any_element(),
                    None => ui::disabled_button("ugc-fit-generate", "Generate Accessory")
                        .into_any_element(),
                    Some(g) if matches!(g.upload, Job::Ready(_)) => ui::icon_button(
                        "ugc-fit-upload",
                        "cloud-upload",
                        "Upload Item\u{2026}",
                        Weight::Primary,
                        false,
                    )
                    .on_click(cx.listener(|v, _, _, cx| v.open_upload(cx)))
                    .into_any_element(),
                    Some(_) => ui::disabled_button("ugc-fit-upload", "Upload Item\u{2026}")
                        .into_any_element(),
                },
                _ => {
                    let next = Step::ALL[index + 1];
                    if next.reachable(self.source.is_some()) {
                        ui::button(
                            "ugc-fit-next",
                            format!("Next: {}", next.label()),
                            Weight::Primary,
                            false,
                        )
                        .child(ui::icon("arrow-right", 13.))
                        .on_click(cx.listener(move |v, _, _, cx| v.go(next, cx)))
                        .into_any_element()
                    } else {
                        ui::disabled_button("ugc-fit-next", "Pick an item first").into_any_element()
                    }
                }
            };
        h_flex()
            .flex_none()
            .gap(px(8.))
            .px(px(PAD))
            .py(px(GAP))
            .border_t_1()
            .border_color(tokens::border())
            .bg(ui::panel())
            .child(
                ui::button("ugc-fit-close", "Close", Weight::Ghost, false)
                    .on_click(|_, window, cx| FittingWindow::close(window, cx)),
            )
            .child(div().flex_1())
            .children(back)
            .child(primary)
    }
}

impl Render for FittingWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.scales_stale) {
            let s = self.current_scales();
            self.inputs.show_scales(&s, window, cx);
        }
        let step = self.step.index();
        let mut cards: Vec<AnyElement> = self
            .step_body(window, cx)
            .into_iter()
            .enumerate()
            .map(|(i, card)| entrance(format!("ugc-fit-card-{step}-{i}"), card, i))
            .collect();
        let n = cards.len();
        cards.push(entrance(
            format!("ugc-fit-card-{step}-{n}"),
            self.check_card(),
            n,
        ));
        v_flex()
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .child(crate::shell::chrome::window_topbar(
                TITLE.into(),
                true,
                FittingWindow::close,
            ))
            .child(
                div()
                    .flex_none()
                    .px(px(PAD))
                    .py(px(GAP))
                    .border_b_1()
                    .border_color(tokens::border())
                    .child(rail(self, cx)),
            )
            .child(
                div()
                    .id("ugc-fit-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        v_flex()
                            .p(px(PAD))
                            .gap(px(GAP))
                            .when_some(self.wear_error.clone(), |el, e| {
                                el.child(note("circle-x", tokens::text_error(), e))
                            })
                            .children(cards),
                    ),
            )
            .child(self.footer(cx))
    }
}
