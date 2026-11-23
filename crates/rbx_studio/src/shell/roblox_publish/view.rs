//! The upload confirmation and the failure dialog, in the launcher's own
//! dialog chrome.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_cloud::PublishMode;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::{verb, Dialog, Shell};

impl Shell {
    pub(in crate::shell) fn roblox_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(match self.roblox.dialog.as_ref()? {
            Dialog::Confirm { mode, target, name } => {
                let (title, go, what) = match mode {
                    PublishMode::Published => (
                        "Publish to Roblox?",
                        "Publish",
                        "This file is uploaded as a new version of the place and published: it becomes the version players join.",
                    ),
                    PublishMode::Saved => (
                        "Save to Roblox?",
                        "Save",
                        "This file is uploaded as a new saved version of the place, without publishing it: players keep joining the version published now.",
                    ),
                };
                let row = |label: &'static str, value: String| {
                    h_flex()
                        .gap(px(12.))
                        .child(
                            ui::text(12., 18.)
                                .w(px(84.))
                                .flex_none()
                                .text_color(tokens::text2())
                                .child(label),
                        )
                        .child(
                            ui::text(12., 18.)
                                .min_w_0()
                                .truncate()
                                .text_color(tokens::text())
                                .child(value),
                        )
                };
                let body = v_flex()
                    .mt(px(14.))
                    .mx(px(20.))
                    .ml(px(78.))
                    .p(px(10.))
                    .gap(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(tokens::border())
                    .bg(ui::panel2())
                    .child(row(
                        "Experience",
                        name.clone()
                            .unwrap_or_else(|| format!("Universe {}", target.universe_id)),
                    ))
                    .child(row("Place", target.place_id.to_string()))
                    .into_any_element();
                // Covers the window like the veil inside it, so the dialog
                // is placed as it would be without the focus wrapper.
                div()
                    .absolute()
                    .inset_0()
                    .track_focus(&self.roblox.focus)
                    .child(ui::dialog(
                        520.,
                        ui::dialog_glyph("cloud-upload", ui::accent(), ui::wash()),
                        title,
                        format!("{what} Your local file isn\u{2019}t changed."),
                        Some(body),
                        vec![
                            cancel(cx),
                            ui::button("roblox-confirm", go, Weight::Primary, false)
                                .on_click(
                                    cx.listener(|shell, _, _, cx| shell.confirm_roblox_upload(cx)),
                                )
                                .into_any_element(),
                        ],
                    ))
                    .into_any_element()
            }
            Dialog::Check { mode, dont_ask, .. } => self.check_dialog(*mode, *dont_ask, cx),
            Dialog::Failed {
                mode,
                target,
                failure,
            } => {
                let (mode, target) = (*mode, *target);
                let body = ui::mono(11.5, 17.)
                    .mt(px(14.))
                    .mx(px(20.))
                    .ml(px(78.))
                    .p(px(10.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(ui::red_line())
                    .bg(ui::red_soft())
                    .text_color(tokens::text())
                    .child(failure.message.clone())
                    .into_any_element();
                ui::dialog(
                    520.,
                    ui::dialog_glyph("circle-alert", ui::red(), ui::red_soft()),
                    format!("{} failed", verb(mode).0),
                    if failure.unchanged {
                        format!(
                            "Place {} was not changed on Roblox. Your local file is untouched.",
                            target.place_id
                        )
                    } else {
                        format!(
                            "The upload to place {} may or may not have been applied \u{2014} check its versions on the Creator Dashboard before trying again. Your local file is untouched.",
                            target.place_id
                        )
                    },
                    Some(body),
                    vec![
                        ui::button(
                            "roblox-relink",
                            "Change place\u{2026}",
                            Weight::Ghost,
                            false,
                        )
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.open_roblox_link(Some(mode), cx)
                        }))
                        .into_any_element(),
                        cancel(cx),
                        ui::button("roblox-retry", "Try again", Weight::Primary, false)
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                shell.start_upload(target, mode, cx)
                            }))
                            .into_any_element(),
                    ],
                )
                .into_any_element()
            }
        })
    }
}

/// How many findings the check's dialog lists before "and N more".
const LISTED: usize = 5;

impl Shell {
    /// The pre-publish check before an upload: a spinner while it runs,
    /// then the worst findings, Review, and Continue.
    fn check_dialog(
        &self,
        mode: PublishMode,
        dont_ask: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::super::prepublish::{checkbox, severity_icon, summary};
        let go = match mode {
            PublishMode::Published => "publishing",
            PublishMode::Saved => "saving",
        };
        let skip = |label: &'static str, cx: &mut Context<Self>| {
            ui::button("roblox-check-continue", label, Weight::Primary, false)
                .on_click(cx.listener(|shell, _, _, cx| shell.continue_past_check(cx)))
                .into_any_element()
        };
        let dont_ask_row = h_flex()
            .id("roblox-check-dont-ask")
            .mt(px(12.))
            .ml(px(78.))
            .mr(px(20.))
            .gap(px(8.))
            .cursor_pointer()
            .on_click(cx.listener(|shell, _, _, cx| shell.toggle_prepublish_dont_ask(cx)))
            .child(checkbox(dont_ask))
            .child(
                ui::text(12., 18.)
                    .text_color(tokens::text2())
                    .child("Don\u{2019}t ask again for this place"),
            )
            .into_any_element();
        let findings = &self.prepublish.findings;
        if self.prepublish.at != Some(self.edit_dom().mutations()) {
            let body = v_flex()
                .child(
                    h_flex()
                        .mt(px(14.))
                        .ml(px(78.))
                        .gap(px(8.))
                        .text_color(tokens::text2())
                        .child(ui::spinner("roblox-check-spinner", 14.))
                        .child(
                            ui::text(12., 18.).child("Checking assets, scripts and parts\u{2026}"),
                        ),
                )
                .child(dont_ask_row)
                .into_any_element();
            return self.check_frame(
                ui::dialog_glyph("list-checks", ui::accent(), ui::wash()),
                format!("Checking before {go}"),
                "Broken asset ids, deprecated API, parts that fall at start, DataStore calls outside pcall, heavy geometry and the Security Audit's findings.".to_string(),
                body,
                vec![cancel(cx), skip("Skip check", cx)],
            );
        }
        let mut worst: Vec<&rbx_audit::Finding> = findings.iter().collect();
        worst.sort_by_key(|finding| finding.severity);
        let more = worst.len().saturating_sub(LISTED);
        let rows = worst.into_iter().take(LISTED).map(|finding| {
            h_flex()
                .gap(px(8.))
                .min_w_0()
                .child(severity_icon(finding.severity, cx))
                .child(
                    ui::text(12., 18.)
                        .flex_none()
                        .text_color(tokens::text())
                        .child(finding.check.title()),
                )
                .child(
                    ui::text(12., 18.)
                        .min_w_0()
                        .truncate()
                        .text_color(tokens::text2())
                        .child(match finding.line {
                            Some(line) => format!("{}:{line}", finding.path),
                            None => finding.path.clone(),
                        }),
                )
        });
        let list = v_flex()
            .mt(px(14.))
            .mx(px(20.))
            .ml(px(78.))
            .p(px(10.))
            .gap(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .children(rows)
            .when(more > 0, |this| {
                this.child(
                    ui::text(12., 18.)
                        .text_color(tokens::text3())
                        .child(format!("and {more} more in the Pre-publish dock")),
                )
            });
        let body = v_flex().child(list).child(dont_ask_row).into_any_element();
        let errors = findings
            .iter()
            .any(|finding| finding.severity == rbx_audit::Severity::High);
        let (glyph, fg, fill) = if errors {
            ("circle-alert", ui::red(), ui::red_soft())
        } else {
            ("triangle-alert", tokens::warning(), ui::wash())
        };
        self.check_frame(
            ui::dialog_glyph(glyph, fg, fill),
            summary(findings),
            format!("The pre-publish check found problems in this place. Review them in the Pre-publish dock, or go on {go} as it is."),
            body,
            vec![
                ui::button("roblox-check-review", "Review", Weight::Secondary, false)
                    .on_click(cx.listener(|shell, _, _, cx| shell.review_prepublish(cx)))
                    .into_any_element(),
                cancel(cx),
                skip("Continue", cx),
            ],
        )
    }

    fn check_frame(
        &self,
        glyph: AnyElement,
        title: String,
        text: String,
        body: AnyElement,
        buttons: Vec<AnyElement>,
    ) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .track_focus(&self.roblox.focus)
            .child(ui::dialog(560., glyph, title, text, Some(body), buttons))
            .into_any_element()
    }
}

fn cancel(cx: &mut Context<Shell>) -> AnyElement {
    ui::button("roblox-cancel", "Cancel", Weight::Secondary, false)
        .on_click(cx.listener(|shell, _, _, cx| {
            shell.close_roblox_dialog();
            cx.notify();
        }))
        .into_any_element()
}
