//! The validator window's layout: controls card, footer and report.

use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::super::import3d::ImportKind;
use super::super::check::Run;
use super::super::{pickers, report, ItemSource};
use super::ValidatorWindow;
use crate::launcher::ui::{self, Weight};
use crate::tokens;

impl ValidatorWindow {
    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let running = matches!(self.check.run, Run::Running);
        let text = match &self.check.run {
            Run::Idle => "Pick something to check".to_string(),
            Run::Running => "Checking\u{2026}".to_string(),
            Run::Done(done) => report::summary(&done.findings),
            Run::Failed(_) => "The check could not run".to_string(),
        };
        let file = match &self.check.source {
            Some(ItemSource::File(path)) => Some(path.clone()),
            _ => None,
        };
        let kind = self.check.kind;
        h_flex()
            .gap(px(12.))
            .pt(px(12.))
            .border_t_1()
            .border_color(tokens::border())
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.))
                    .when(running, |el| el.child(ui::spinner("ugc-check-spin", 14.)))
                    .child(
                        ui::text(12.5, 18.)
                            .text_color(tokens::text2())
                            .truncate()
                            .child(text),
                    ),
            )
            .when(self.check.source.is_some(), |el| {
                el.child(
                    ui::button("ugc-check-again", "Check Again", Weight::Secondary, true)
                        .on_click(cx.listener(|view, _, _, cx| view.restart(cx))),
                )
            })
            .when_some(file, |el, path| {
                let shell = self.shell.clone();
                el.child(
                    ui::button(
                        "ugc-check-import",
                        "Import into Place",
                        Weight::Secondary,
                        true,
                    )
                    .on_click(move |_, _, cx| {
                        let path = path.clone();
                        shell.update(cx, |shell, cx| {
                            shell.import_as(vec![path], ImportKind::Avatar(kind), cx)
                        });
                    }),
                )
            })
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.check.run {
            Run::Done(done) => {
                let subject = self
                    .check
                    .source
                    .as_ref()
                    .map_or_else(|| "The item".to_string(), ItemSource::label);
                let pick = self.picking().then(|| {
                    let view = cx.entity().downgrade();
                    report::Pick {
                        shown: self.shown,
                        on_pick: Rc::new(move |index, _, cx| {
                            let _ = view.update(cx, |view, cx| view.show(index, cx));
                        }),
                    }
                });
                report::findings_view("ugc-check-report", &done.findings, &subject, pick)
            }
            Run::Failed(msg) => h_flex()
                .items_start()
                .gap(px(12.))
                .p(px(12.))
                .rounded(px(8.))
                .bg(tokens::error_soft())
                .child(ui::icon("circle-x", 18.).text_color(tokens::text_error()))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(4.))
                        .child(
                            ui::text(13., 18.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(tokens::text_error())
                                .child("Could not check this item"),
                        )
                        .child(
                            ui::text(12.5, 18.)
                                .text_color(tokens::text())
                                .child(msg.clone()),
                        ),
                )
                .into_any_element(),
            Run::Running => h_flex()
                .gap(px(8.))
                .p(px(12.))
                .child(ui::spinner("ugc-check-body-spin", 16.))
                .child(
                    ui::text(12.5, 18.)
                        .text_color(tokens::text2())
                        .child("Reading the item\u{2026}"),
                )
                .into_any_element(),
            Run::Idle => ui::text(12.5, 18.)
                .p(px(12.))
                .text_color(tokens::text2())
                .child("Use the selection or choose a file to see how it measures up.")
                .into_any_element(),
        }
    }
}

impl Render for ValidatorWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let sel = this.clone();
        let file = this.clone();
        let pick = this.clone();
        let busy = matches!(self.check.run, Run::Running);
        let controls = v_flex()
            .flex_none()
            .gap(px(12.))
            .p(px(16.))
            .rounded(px(8.))
            .bg(ui::panel())
            .border_1()
            .border_color(tokens::border())
            .child(pickers::source_card(
                "ugc-check-source",
                self.check.source.as_ref(),
                move |_, cx| sel.update(cx, |v, cx| v.use_selection(cx)),
                move |_, cx| file.update(cx, |v, cx| v.choose_file(cx)),
            ))
            .child(pickers::kind_picker(
                "ugc-check-kind",
                self.check.kind,
                busy,
                move |kind, _, cx| {
                    pick.update(cx, |v, cx| {
                        if v.check.kind != kind {
                            v.check.kind = kind;
                            v.restart(cx);
                        }
                    })
                },
            ))
            .child(self.footer(cx));
        v_flex()
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .child(crate::shell::chrome::window_topbar(
                "Check Avatar Item".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(16.))
                    .gap(px(12.))
                    .child(controls)
                    .child(
                        div()
                            .id("ugc-check-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .child(self.body(cx)),
                    ),
            )
    }
}
