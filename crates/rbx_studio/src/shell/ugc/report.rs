//! The findings list both windows show, grouped by severity.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_ugc::{Finding, Severity};

use crate::launcher::ui::{self, Weight};
use crate::tokens;

fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn count(findings: &[Finding], severity: Severity) -> usize {
    findings.iter().filter(|f| f.severity == severity).count()
}

/// "2 errors, 1 warning" (empty findings: "Passes every documented check").
pub(super) fn summary(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "Passes every documented check".to_string();
    }
    let parts: Vec<String> = [
        (Severity::Error, "error", "errors"),
        (Severity::Warning, "warning", "warnings"),
        (Severity::Note, "note", "notes"),
    ]
    .iter()
    .filter_map(|(s, one, many)| {
        let n = count(findings, *s);
        (n > 0).then(|| counted(n, one, many))
    })
    .collect();
    parts.join(", ")
}

fn look(severity: Severity) -> (&'static str, Rgba, Rgba) {
    match severity {
        Severity::Error => ("circle-x", ui::red(), ui::red_soft()),
        Severity::Warning => ("triangle-alert", tokens::warning(), ui::wash()),
        Severity::Note => ("info", tokens::text2(), ui::wash_faint()),
    }
}

fn caption(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "Errors \u{b7} Roblox refuses the item",
        Severity::Warning => "Warnings \u{b7} against the spec",
        Severity::Note => "Notes \u{b7} not checked",
    }
}

fn chip(text: String, fg: Rgba, fill: Rgba) -> Div {
    h_flex()
        .h(px(24.))
        .px(px(10.))
        .rounded(px(6.))
        .bg(fill)
        .text_color(fg)
        .child(
            ui::text(12., 16.)
                .font_weight(FontWeight::SEMIBOLD)
                .child(text),
        )
}

/// What picking a finding does, given its index in the findings.
pub(super) type OnPick = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// Findings that can be picked to show them on the model, and the one
/// shown.
pub(super) struct Pick {
    pub(super) shown: Option<usize>,
    pub(super) on_pick: OnPick,
}

fn row(
    id: &'static str,
    index: usize,
    order: usize,
    finding: &Finding,
    pick: Option<&Pick>,
) -> AnyElement {
    let (glyph, fg, fill) = look(finding.severity);
    let source = finding.rule.source();
    let measure = match (&finding.measured, &finding.limit) {
        (None, None) => None,
        (m, l) => {
            let mut parts = Vec::new();
            if let Some(m) = m {
                parts.push(format!("Measured {m}"));
            }
            if let Some(l) = l {
                parts.push(format!("Limit {l}"));
            }
            Some(parts.join(" \u{b7} "))
        }
    };
    let shown = pick.is_some_and(|p| p.shown == Some(index));
    let card = h_flex()
        .id(SharedString::from(format!("{id}-card-{index}")))
        .items_start()
        .gap(px(12.))
        .p(px(12.))
        .rounded(px(8.))
        .bg(ui::panel())
        .border_1()
        .border_color(if shown {
            tokens::accent_line()
        } else {
            tokens::border()
        })
        .when_some(pick.map(|p| p.on_pick.clone()), |el, on_pick| {
            el.cursor_pointer()
                .hover(|s| s.bg(tokens::hover_faint()))
                .on_click(move |_, window, cx| on_pick(index, window, cx))
        })
        .child(
            div()
                .flex_none()
                .size(px(24.))
                .rounded_full()
                .bg(fill)
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon(glyph, 14.).text_color(fg)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(4.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .flex_wrap()
                        .child(
                            ui::text(13., 18.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(tokens::text_strong())
                                .child(finding.rule.title()),
                        )
                        .child(
                            ui::mono(11.5, 16.)
                                .text_color(tokens::text2())
                                .child(finding.subject.clone()),
                        ),
                )
                .child(
                    ui::text(12.5, 18.)
                        .text_color(tokens::text())
                        .child(finding.message.clone()),
                )
                .when_some(measure, |el, m| {
                    el.child(ui::mono(11., 16.).text_color(tokens::text2()).child(m))
                }),
        )
        .child(
            ui::external_button(
                SharedString::from(format!("{id}-spec-{index}")),
                "Spec",
                Weight::Ghost,
                true,
            )
            .flex_none()
            .on_click(move |_, _, cx| cx.open_url(source)),
        );
    if tokens::reduced_motion() {
        return card.into_any_element();
    }
    let delay = 30 * order as u64;
    let total = 220 + delay;
    let start = delay as f32 / total as f32;
    card.with_animation(
        SharedString::from(format!("{id}-row-{index}")),
        Animation::new(Duration::from_millis(total)).with_easing(tokens::easing_soft),
        move |el, t| {
            let p = ((t - start) / (1. - start)).clamp(0., 1.);
            el.opacity(p).mt(px(8. * (1. - p)))
        },
    )
    .into_any_element()
}

fn all_clear(subject: &str) -> AnyElement {
    v_flex()
        .items_center()
        .gap(px(12.))
        .p(px(24.))
        .rounded(px(8.))
        .bg(ui::panel())
        .border_1()
        .border_color(tokens::border())
        .child(
            div()
                .size(px(56.))
                .rounded_full()
                .bg(ui::green_soft())
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon("badge-check", 28.).text_color(ui::green())),
        )
        .child(
            ui::text(15., 20.)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens::text_strong())
                .child("Passes every documented check"),
        )
        .child(
            ui::text(12.5, 18.)
                .text_color(tokens::text2())
                .text_center()
                .child(format!(
                    "{subject} is within Roblox\u{2019}s published limits. Roblox runs its own checks when you submit."
                )),
        )
        .into_any_element()
}

/// Findings grouped Errors / Warnings / Notes, each row saying what is wrong,
/// the measured value against the limit, and a link to the spec page.
/// `subject` names the checked item in the all-clear state.
/// With `pick`, a row is clicked to show that finding on the model.
pub(super) fn findings_view(
    id: &'static str,
    findings: &[Finding],
    subject: &str,
    pick: Option<Pick>,
) -> AnyElement {
    if findings.is_empty() {
        return all_clear(subject);
    }
    let errors = count(findings, Severity::Error);
    let warnings = count(findings, Severity::Warning);
    let notes = count(findings, Severity::Note);
    let mut root = v_flex().gap(px(12.)).child(
        h_flex()
            .gap(px(8.))
            .flex_wrap()
            .when(errors > 0, |el| {
                el.child(chip(
                    counted(errors, "error", "errors"),
                    ui::red(),
                    ui::red_soft(),
                ))
            })
            .when(warnings > 0, |el| {
                el.child(chip(
                    counted(warnings, "warning", "warnings"),
                    tokens::warning(),
                    ui::wash(),
                ))
            })
            .when(notes > 0, |el| {
                el.child(chip(
                    counted(notes, "note", "notes"),
                    tokens::text2(),
                    ui::wash_faint(),
                ))
            }),
    );
    let mut order = 0;
    for severity in [Severity::Error, Severity::Warning, Severity::Note] {
        let group: Vec<(usize, &Finding)> = findings
            .iter()
            .enumerate()
            .filter(|(_, f)| f.severity == severity)
            .collect();
        if group.is_empty() {
            continue;
        }
        let mut section = v_flex().gap(px(8.)).child(
            ui::text(11.5, 16.)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens::text3())
                .child(caption(severity)),
        );
        for (index, finding) in group {
            section = section.child(row(id, index, order, finding, pick.as_ref()));
            order += 1;
        }
        root = root.child(section);
    }
    root.into_any_element()
}
