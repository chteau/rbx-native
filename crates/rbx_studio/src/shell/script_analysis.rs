//! The Script Analysis dock: every problem `luau-lsp` reports across the
//! place, grouped under the script it is in. Clicking a problem opens that
//! script with the cursor on it.
//!
//! Studio's own Script Analysis runs its own static pass; this one is the
//! same `luau-lsp` pull the editor's squiggles come from (see
//! `shell::luau_lsp`), so the two always agree.

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use lsp_types::{Diagnostic, DiagnosticSeverity};
use rbx_dom::{Ref, WeakDom};

use crate::luau_lsp::diagnostics::{self, Counts};
use crate::tokens;

use super::chrome;
use super::layout::Panel;
use super::luau_lsp::Status;
use super::menu::{self, MenuId};
use super::Shell;

impl Shell {
    pub(super) fn script_analysis_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        // Showing the dock is asking for the analysis, tabs open or not.
        self.ensure_luau_lsp(cx);
        self.notice_luau_lsp_dom_change(cx);

        let overflow = menu::dropdown(
            self,
            MenuId::ScriptAnalysisOverflow,
            chrome::Trigger::new(chrome::dock_options_button(
                "script-analysis-overflow",
                IconName::Ellipsis,
                16.,
                "Script Analysis dock options",
            )),
            self.move_items(Panel::ScriptAnalysis),
            cx,
        );

        let counts = diagnostics::counts(&self.lsp.problems);
        let failed = matches!(self.lsp.status, Status::Failed(_));
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
                    .text_color(match failed {
                        true => tokens::text_error(),
                        false => tokens::text2(),
                    })
                    .child(summary(&self.lsp.status, counts)),
            )
            .when(failed, |this| {
                this.child(
                    div()
                        .id("script-analysis-retry")
                        .px_2()
                        .rounded_sm()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text())
                        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                        .on_click(cx.listener(|shell, _, _, cx| shell.restart_luau_lsp(cx)))
                        .child("Retry"),
                )
            });

        let mut list = v_flex().w_full();
        for (script, problems) in &self.lsp.problems {
            list = list.child(script_row(&self.dom, *script, problems.len()));
            for (index, problem) in problems.iter().enumerate() {
                list = list.child(problem_row(*script, index, problem, cx));
            }
        }

        let body = v_flex().size_full().child(header).child(
            div()
                .id("script-analysis-list")
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(&self.lsp.scroll)
                .child(list)
                .vertical_scrollbar(&self.lsp.scroll),
        );
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }
}

fn script_row(dom: &WeakDom, script: Ref, count: usize) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_2()
        .px_2()
        .pt_1()
        .text_size(tokens::text_xs())
        .child(
            div()
                .text_color(tokens::text_strong())
                .child(full_name(dom, script)),
        )
        .child(div().text_color(tokens::text3()).child(count.to_string()))
}

fn problem_row(
    script: Ref,
    index: usize,
    problem: &Diagnostic,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let (icon, color) = match problem.severity {
        Some(DiagnosticSeverity::ERROR) => (IconName::CircleX, cx.theme().danger),
        Some(DiagnosticSeverity::WARNING) => (IconName::TriangleAlert, cx.theme().warning),
        _ => (IconName::Info, cx.theme().muted_foreground),
    };
    let start = problem.range.start;
    let target = problem.clone();
    h_flex()
        .id((
            "script-analysis-problem",
            (script.value() as usize) << 16 | index,
        ))
        .w_full()
        .gap_2()
        // Indented under its script's row.
        .pl_6()
        .pr_2()
        .py_0p5()
        .text_size(tokens::text_xs())
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.reveal_problem(script, &target, window, cx);
        }))
        .child(Icon::new(icon).small().text_color(color))
        .child(div().flex_none().text_color(tokens::text3()).child(format!(
            "Ln {}, Col {}",
            start.line + 1,
            start.character + 1
        )))
        .child(
            div()
                .flex_1()
                .text_color(tokens::text())
                .child(problem.message.clone()),
        )
}

/// What the header says: where the server is, or what it found.
fn summary(status: &Status, counts: Counts) -> String {
    match status {
        Status::Off | Status::Starting => "Starting luau-lsp…".into(),
        Status::Failed(reason) => reason.clone(),
        Status::Ready(_) => count_summary(counts),
    }
}

fn count_summary(counts: Counts) -> String {
    if counts == Counts::default() {
        return "No problems found".into();
    }
    [
        (counts.errors, "error"),
        (counts.warnings, "warning"),
        (counts.notes, "note"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, noun)| format!("{n} {noun}{}", if n == 1 { "" } else { "s" }))
    .collect::<Vec<_>>()
    .join(", ")
}

/// `Instance:GetFullName()`: the dotted path from the service down, which is
/// how Studio's own Script Analysis names a script.
fn full_name(dom: &WeakDom, script: Ref) -> String {
    let mut names = Vec::new();
    let mut at = Some(script);
    while let Some(reference) = at {
        let Some(instance) = dom.get(reference) else {
            break;
        };
        names.push(instance.name());
        at = dom.parent(reference);
    }
    names.reverse();
    names.join(".")
}

#[cfg(test)]
mod tests {
    use rbx_dom::WeakDom;

    use super::{count_summary, full_name, Counts};

    #[test]
    fn a_script_is_named_by_its_path_from_the_service() {
        let mut dom = WeakDom::new();
        let service = dom.new_instance("ServerScriptService", "ServerScriptService", None);
        let folder = dom.new_instance("Folder", "Systems", Some(service));
        let script = dom.new_instance("Script", "Combat", Some(folder));
        assert_eq!(
            full_name(&dom, script),
            "ServerScriptService.Systems.Combat"
        );
    }

    #[test]
    fn the_summary_counts_only_what_there_is() {
        let counts = |errors, warnings, notes| Counts {
            errors,
            warnings,
            notes,
        };
        assert_eq!(count_summary(counts(0, 0, 0)), "No problems found");
        assert_eq!(count_summary(counts(1, 2, 0)), "1 error, 2 warnings");
        assert_eq!(count_summary(counts(0, 0, 3)), "3 notes");
    }
}
