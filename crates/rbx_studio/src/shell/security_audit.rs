//! The Security Audit dock: `rbx_audit::audit` run over the whole place,
//! its findings grouped under the script or remote they concern. Clicking a
//! line finding opens the script at that line; a place-level finding selects
//! the remote in the Explorer.
//!
//! Studio has no equivalent. The audit is cached against the DOM's mutation
//! count, so it re-runs only after an edit (or Re-run), not on every frame,
//! and off the UI thread: a large place takes a few hundred milliseconds.
//! It audits a slim copy of the DOM (`rbx_audit::slim_copy`) once edits have
//! been quiet for [`QUIET`], so a drag doesn't queue one per frame.
//!
//! `RBX_STUDIO_SECURITY_AUDIT=1` opens the dock at startup: a screenshot aid
//! like the other `RBX_STUDIO_*` variables.

use crate::shell::chrome::ScrollbarY as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::*;
use rbx_audit::{Finding, Severity};

use std::ops::Range;
use std::time::Duration;

use crate::tokens;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::Shell;

/// Read once at startup; documented in this module's doc comment.
const SECURITY_AUDIT_VARIABLE: &str = "RBX_STUDIO_SECURITY_AUDIT";

/// How long the DOM has to stay unchanged before an edit re-audits it.
const QUIET: Duration = Duration::from_millis(300);

/// The last audit and the DOM state it was taken at.
#[derive(Default)]
pub(super) struct AuditState {
    findings: Vec<Finding>,
    at: Option<u64>,
    /// The DOM state an audit is queued or running for; dropping the task
    /// cancels it.
    pending: Option<(u64, Task<()>)>,
    scroll: ScrollHandle,
}

impl Shell {
    /// `RBX_STUDIO_SECURITY_AUDIT=1`: see this module's doc comment.
    pub(super) fn apply_debug_security_audit(&mut self, cx: &mut Context<Self>) {
        if std::env::var_os(SECURITY_AUDIT_VARIABLE).is_some() {
            self.set_panel_open(Panel::SecurityAudit, true, cx);
        }
    }

    pub(super) fn security_audit_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        self.queue_audit(cx);

        let overflow = menu::dropdown(
            self,
            MenuId::SecurityAuditOverflow,
            chrome::dock_options_trigger(
                "security-audit-overflow",
                IconName::Ellipsis,
                16.,
                "Security Audit dock options",
            ),
            self.move_items(Panel::SecurityAudit),
            cx,
        );

        let count = |severity| {
            self.audit
                .findings
                .iter()
                .filter(|finding| finding.severity == severity)
                .count()
        };
        let header = h_flex()
            .w_full()
            .flex_none()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(tokens::border())
            .child(
                div()
                    .flex_1()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text2())
                    .child(match self.audit.at {
                        None => "Auditing\u{2026}".to_string(),
                        Some(_) => summary(
                            count(Severity::High),
                            count(Severity::Medium),
                            count(Severity::Low),
                        ),
                    }),
            )
            .child(
                div()
                    .id("security-audit-rerun")
                    .px_2()
                    .rounded_sm()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text())
                    .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.audit.at = None;
                        shell.audit.pending = None;
                        cx.notify();
                    }))
                    .child("Re-run"),
            );

        let mut list = v_flex().w_full();
        let mut index = 0;
        while index < self.audit.findings.len() {
            let path = &self.audit.findings[index].path;
            let run = self.audit.findings[index..]
                .iter()
                .take_while(|finding| &finding.path == path)
                .count();
            list = list.child(path_row(path, run));
            let findings = self.audit.findings.iter().enumerate();
            for (offset, finding) in findings.skip(index).take(run) {
                list = list.child(finding_row(offset, finding, cx));
            }
            index += run;
        }

        let body = v_flex().size_full().child(header).child(
            div()
                .id("security-audit-list")
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(&self.audit.scroll)
                .pr(super::chrome::SCROLLBAR_GUTTER)
                .child(list)
                .scrollbar_y(&self.audit.scroll),
        );
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }
}

impl Shell {
    /// Audits the place in the background when it changed since the last
    /// audit, after [`QUIET`] unless there are no findings to show yet.
    fn queue_audit(&mut self, cx: &mut Context<Self>) {
        let stamp = self.edit_dom().mutations();
        if self.audit.at == Some(stamp) || self.audit.pending.as_ref().is_some_and(|p| p.0 == stamp)
        {
            return;
        }
        let wait = self.audit.at.is_some();
        let task = cx.spawn(async move |shell, cx| {
            if wait {
                cx.background_executor().timer(QUIET).await;
            }
            // Edited again while waiting: the next render queues that one.
            let Ok(Some(copy)) = shell.update(cx, |shell, _| {
                (shell.edit_dom().mutations() == stamp)
                    .then(|| rbx_audit::slim_copy(shell.edit_dom()))
            }) else {
                return;
            };
            let findings = cx
                .background_spawn(async move {
                    let mut findings = rbx_audit::audit(&copy);
                    // One group per path in the dock; a stable sort keeps
                    // the worst finding first within each.
                    findings.sort_by(|a, b| a.path.cmp(&b.path));
                    findings
                })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                shell.audit.findings = findings;
                shell.audit.at = Some(stamp);
                shell.audit.pending = None;
                cx.notify();
            });
        });
        self.audit.pending = Some((stamp, task));
    }
}

fn path_row(path: &str, count: usize) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_2()
        .px_2()
        .pt_1()
        .text_size(tokens::text_xs())
        .child(
            div()
                .text_color(tokens::text_strong())
                .child(path.to_owned()),
        )
        .child(div().text_color(tokens::text3()).child(count.to_string()))
}

fn finding_row(index: usize, finding: &Finding, cx: &mut Context<Shell>) -> impl IntoElement {
    let (icon, color) = match finding.severity {
        Severity::High => (IconName::CircleX, cx.theme().danger),
        Severity::Medium => (IconName::TriangleAlert, cx.theme().warning),
        Severity::Low => (IconName::Info, cx.theme().muted_foreground),
    };
    let (instance, line) = (finding.instance, finding.line);
    h_flex()
        .id(("security-audit-finding", index))
        .w_full()
        .gap_2()
        // Indented under its path's row.
        .pl_6()
        .pr_2()
        .py_0p5()
        .text_size(tokens::text_xs())
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .on_click(cx.listener(move |shell, _, window, cx| match line {
            Some(line) => {
                let pos = lsp_types::Position::new(line as u32 - 1, 0);
                shell.reveal(instance, lsp_types::Range::new(pos, pos), window, cx);
            }
            None => shell.select(instance, cx),
        }))
        .child(Icon::new(icon).small().text_color(color))
        .children(line.map(|line| {
            div()
                .flex_none()
                .text_color(tokens::text3())
                .child(format!("Ln {line}"))
        }))
        .child(
            div()
                .flex_none()
                .text_color(tokens::text_strong())
                .child(finding.check.title()),
        )
        .child(div().flex_1().child(message(&finding.message)))
}

/// A finding's message with its `code` spans set in the code font and their
/// backticks dropped.
fn message(text: &str) -> StyledText {
    let (plain, spans) = code_spans(text);
    let runs = spans
        .into_iter()
        .map(|(range, code)| TextRun {
            len: range.len(),
            font: font(if code {
                tokens::font_mono()
            } else {
                tokens::font_ui()
            }),
            color: if code {
                tokens::text_strong()
            } else {
                tokens::text()
            }
            .into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        })
        .collect();
    StyledText::new(plain).with_runs(runs)
}

/// `text` without its backticks, and the ranges of that text that were
/// between them (`true`) or not. An unpaired backtick is kept as it is.
fn code_spans(text: &str) -> (String, Vec<(Range<usize>, bool)>) {
    let parts: Vec<&str> = text.split('`').collect();
    if parts.len().is_multiple_of(2) {
        return (text.to_string(), vec![(0..text.len(), false)]);
    }
    let mut plain = String::new();
    let mut spans = Vec::new();
    for (i, part) in parts.into_iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let start = plain.len();
        plain.push_str(part);
        spans.push((start..plain.len(), i % 2 == 1));
    }
    (plain, spans)
}

/// What the header says: the counts that are not zero, or that all is well.
fn summary(high: usize, medium: usize, low: usize) -> String {
    let parts: Vec<String> = [(high, "high"), (medium, "medium"), (low, "low")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, label)| format!("{n} {label}"))
        .collect();
    if parts.is_empty() {
        return "No problems found".into();
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::{code_spans, summary};

    #[test]
    fn backticks_become_code_spans() {
        let (plain, spans) = code_spans("The `Remotes.Pay` handler sets `x.Value`.");
        assert_eq!(plain, "The Remotes.Pay handler sets x.Value.");
        let code: Vec<&str> = spans
            .iter()
            .filter(|(_, code)| *code)
            .map(|(range, _)| &plain[range.clone()])
            .collect();
        assert_eq!(code, ["Remotes.Pay", "x.Value"]);
        assert_eq!(
            spans.iter().map(|(r, _)| r.len()).sum::<usize>(),
            plain.len()
        );
    }

    #[test]
    fn an_unpaired_backtick_stays_text() {
        let (plain, spans) = code_spans("a ` b");
        assert_eq!(plain, "a ` b");
        assert_eq!(spans, [(0..5, false)]);
    }

    #[test]
    fn the_summary_names_only_the_severities_there_are() {
        assert_eq!(summary(0, 0, 0), "No problems found");
        assert_eq!(summary(2, 0, 1), "2 high, 1 low");
        assert_eq!(summary(1, 3, 4), "1 high, 3 medium, 4 low");
    }
}
