//! The Tests dock: every spec file in the place laid out as a tree of
//! `describe`/`it` nodes, each with its last result, and buttons to run all of
//! it or any one file, group or test.
//!
//! The tree comes from `rbx_lua::test_runner::plan`, which reads the specs
//! without running them; runs go through the same runner `rbxlua test` uses, on a
//! background thread over a copy of the place, so the editor never waits on a
//! spec. Loren's `return function(t)` specs go through `rbx_loren::specs`
//! instead, Loren's own runner, and land in the same tree. With the Local
//! Run beta on, integration specs (`*.integration.spec`) are listed too and
//! run on a Local Run session (`rbx_sim::integration`); with it off they
//! are left out, and the plain runner works as before.
//!
//! Selecting a row shows what that test printed and how it failed in the
//! pane under the tree; a file row shows everything the file printed.

use std::collections::{HashMap, HashSet};

use crate::shell::chrome::ScrollbarY as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;
use rbx_loren::specs as loren;
use rbx_lua::test_runner::{
    self, FilePlan, NodeKind, Options, PlanNode, SpecFile, Status as TestStatus, TestResult,
};
use rbx_lua::{LineKind, Output};

use crate::tokens;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::Shell;

type Key = (Ref, Vec<String>);

#[derive(Default)]
pub(super) struct TestsDock {
    plans: Vec<FilePlan>,
    /// The history revision `plans` was read from.
    planned: Option<u64>,
    planning: bool,
    results: HashMap<Key, TestResult>,
    running: bool,
    rerun_on_save: bool,
    collapsed: HashSet<Key>,
    /// What each file printed in its last run.
    outputs: HashMap<Ref, Output>,
    /// The row the output pane shows.
    selected: Option<Key>,
    pub(super) scroll: ScrollHandle,
    output_scroll: ScrollHandle,
    error: Option<String>,
}

impl Shell {
    pub(super) fn tests_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        self.plan_tests(cx);

        let overflow = menu::dropdown(
            self,
            MenuId::TestsOverflow,
            chrome::dock_options_trigger(
                "tests-overflow",
                IconName::Ellipsis,
                16.,
                "Tests dock options",
            ),
            self.move_items(Panel::Tests),
            cx,
        );

        let dock = &self.tests;
        let (mut passed, mut failed, mut skipped) = (0, 0, 0);
        for result in dock.results.values() {
            match result.status {
                TestStatus::Passed => passed += 1,
                TestStatus::Failed => failed += 1,
                TestStatus::Skipped => skipped += 1,
            }
        }
        let text = summary(
            dock.running,
            dock.plans.is_empty() && dock.error.is_none(),
            (passed, failed, skipped),
        );
        let rerun = dock.rerun_on_save;
        let running = dock.running;

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
                    .child(text),
            )
            .child(
                h_flex()
                    .id("tests-rerun-on-save")
                    .gap_1()
                    .px_2()
                    .rounded_sm()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text())
                    .when(rerun, |this| this.bg(tokens::accent_soft()))
                    .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.tests.rerun_on_save = !shell.tests.rerun_on_save;
                        cx.notify();
                    }))
                    .when(rerun, |this| {
                        this.child(
                            Icon::new(IconName::Check)
                                .small()
                                .text_color(tokens::check_on()),
                        )
                    })
                    .child("Rerun on save"),
            )
            .child(
                h_flex()
                    .id("tests-run-all")
                    .gap_1()
                    .px_2()
                    .rounded_sm()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text())
                    .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                    .when(!running, |this| {
                        this.on_click(cx.listener(|shell, _, _, cx| shell.run_all_tests(cx)))
                    })
                    .child(Icon::new(IconName::Play).small().text_color(tokens::text()))
                    .child("Run all"),
            );

        let body = v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("tests-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.tests.scroll)
                    .pr(chrome::SCROLLBAR_GUTTER)
                    .child(self.tests_tree(cx))
                    .scrollbar_y(&self.tests.scroll),
            )
            .children(self.tests_output_pane());
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    fn tests_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = &self.tests;
        let mut list = v_flex().w_full();
        let mut n = 0usize;
        if let Some(error) = &dock.error {
            list = list.child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text_error())
                    .child(error.clone()),
            );
        }
        for plan in &dock.plans {
            let file = plan.file.clone();
            let module = file.module;
            let file_key = (module, Vec::new());
            let collapsed = dock.collapsed.contains(&file_key);
            let load_failure = dock.results.get(&file_key);
            let failure_text = plan
                .error
                .clone()
                .or_else(|| load_failure.and_then(|r| r.message.clone()));

            let status = match &failure_text {
                Some(_) => Some(TestStatus::Failed),
                None => status_of(&dock.results, module, &[]),
            };
            let chevron = match collapsed {
                true => IconName::ChevronRight,
                false => IconName::ChevronDown,
            };
            let toggle_key = file_key.clone();
            let run_file = file.clone();
            list = list.child(row(
                n,
                0,
                Some(chevron),
                status,
                file.name.clone(),
                None,
                dock.running,
                dock.selected.as_ref() == Some(&file_key),
                cx,
                move |shell, _, cx| {
                    let dock = &mut shell.tests;
                    dock.selected = Some(toggle_key.clone());
                    if !dock.collapsed.remove(&toggle_key) {
                        dock.collapsed.insert(toggle_key.clone());
                    }
                    cx.notify();
                },
                move |shell, cx| shell.run_tests(vec![run_file.clone()], Options::default(), cx),
            ));
            n += 1;
            if let Some(text) = failure_text {
                let at = load_failure.and_then(|r| r.location.clone());
                list = list.child(failure_row(n, 1, text, at, module, 1, cx));
                n += 1;
            }
            if collapsed {
                continue;
            }
            for node in &plan.nodes {
                let (element, used) = node_rows(n, module, &file, node, dock, cx);
                list = list.child(element);
                n += used;
            }
        }
        list
    }

    fn plan_tests(&mut self, cx: &mut Context<Self>) {
        let revision = self.history.revision();
        if self.tests.planning || self.tests.planned == Some(revision) {
            return;
        }
        self.tests.planning = true;
        let dom = self.edit_dom().clone();
        let database = self.database.clone();
        cx.spawn(async move |this, cx| {
            let planned = cx
                .background_spawn(async move {
                    let files = discover_specs(&dom, local_run_beta())?;
                    Ok::<_, String>(
                        files
                            .iter()
                            .map(|file| loren::plan_mixed(&dom, file, &database))
                            .collect::<Vec<_>>(),
                    )
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                let dock = &mut shell.tests;
                dock.planning = false;
                dock.planned = Some(revision);
                match planned {
                    Ok(plans) => {
                        dock.plans = plans;
                        dock.error = None;
                    }
                    Err(error) => {
                        dock.plans.clear();
                        dock.error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn run_all_tests(&mut self, cx: &mut Context<Self>) {
        let files = self.tests.plans.iter().map(|p| p.file.clone()).collect();
        self.run_tests(files, Options::default(), cx);
    }

    fn run_tests(&mut self, files: Vec<SpecFile>, options: Options, cx: &mut Context<Self>) {
        if self.tests.running || files.is_empty() {
            return;
        }
        self.tests.running = true;
        let dom = self.edit_dom().clone();
        let database = self.database.clone();
        let only = options.only.clone();
        let modules: Vec<Ref> = files.iter().map(|f| f.module).collect();
        cx.spawn(async move |this, cx| {
            let reports = cx
                .background_spawn(async move {
                    let (integration, plain): (Vec<_>, Vec<_>) = files
                        .into_iter()
                        .partition(|f| test_runner::is_integration(&f.name));
                    let mut reports = loren::run_mixed(&dom, &plain, &database, &options).files;
                    if !integration.is_empty() {
                        let config = rbx_sim::integration::Config::default();
                        reports.extend(
                            rbx_sim::integration::run(
                                &dom,
                                &integration,
                                &database,
                                &options,
                                &config,
                            )
                            .files,
                        );
                    }
                    reports
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                let dock = &mut shell.tests;
                dock.results.retain(|(module, path), _| {
                    !modules.contains(module)
                        || only.as_ref().is_some_and(|only| !path.starts_with(only))
                });
                for report in reports {
                    dock.outputs.insert(report.file.module, report.output);
                    for result in report.results {
                        dock.results
                            .insert((report.file.module, result.path.clone()), result);
                    }
                }
                dock.running = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// `RBX_STUDIO_TESTS=1|run`: see the crate's doc comment.
    pub(super) fn apply_debug_tests(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var("RBX_STUDIO_TESTS") else {
            return;
        };
        self.set_panel_open(Panel::Tests, true, cx);
        if spec == "run" {
            let files = discover_specs(self.edit_dom(), local_run_beta()).unwrap_or_default();
            self.run_tests(files, Options::default(), cx);
        }
    }

    pub(super) fn rerun_tests_on_save(&mut self, cx: &mut Context<Self>) {
        if !self.tests.rerun_on_save {
            return;
        }
        self.tests.planned = None;
        match discover_specs(self.edit_dom(), local_run_beta()) {
            Ok(files) => self.run_tests(files, Options::default(), cx),
            Err(error) => {
                self.tests.error = Some(error);
                cx.notify();
            }
        }
    }

    fn reveal_spec_line(
        &mut self,
        script: Ref,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = lsp_types::Position::new(line.saturating_sub(1), 0);
        self.reveal(script, lsp_types::Range::new(at, at), window, cx);
    }
}

/// The rows for one plan node, and how many ids they used.
fn local_run_beta() -> bool {
    crate::beta::enabled(super::local_run::BETA_ID)
}

/// The specs the dock lists: the integration specs too when Local Run
/// (`local_run`, its beta) can run them.
pub(super) fn discover_specs(
    dom: &rbx_dom::WeakDom,
    local_run: bool,
) -> Result<Vec<SpecFile>, String> {
    let mut files = test_runner::discover(dom, &[])?;
    if local_run {
        files.extend(test_runner::discover_integration(dom, &[])?);
    }
    Ok(files)
}

fn node_rows(
    n: usize,
    module: Ref,
    file: &SpecFile,
    node: &PlanNode,
    dock: &TestsDock,
    cx: &mut Context<Shell>,
) -> (impl IntoElement, usize) {
    let depth = node.path.len();
    let exact = dock.results.get(&(module, node.path.clone()));
    let status = match node.skip {
        true => Some(TestStatus::Skipped),
        false => status_of(&dock.results, module, &node.path),
    };
    let name = node.path.last().cloned().unwrap_or_default();
    let duration = match node.kind {
        NodeKind::Test => exact.map(|r| r.duration.as_millis()),
        NodeKind::Describe => None,
    };
    let line = node.line;
    let only = Options {
        only: Some(node.path.clone()),
        ..Options::default()
    };
    let run_file = file.clone();
    let key = (module, node.path.clone());
    let selected = dock.selected.as_ref() == Some(&key);
    let mut rows = v_flex().w_full().child(row(
        n,
        depth,
        None,
        status,
        name,
        duration,
        dock.running,
        selected,
        cx,
        move |shell, window, cx| {
            shell.tests.selected = Some(key.clone());
            cx.notify();
            if let Some(line) = line {
                shell.reveal_spec_line(module, line, window, cx);
            }
        },
        move |shell, cx| shell.run_tests(vec![run_file.clone()], only.clone(), cx),
    ));
    let mut used = 1;
    if let Some(result) = exact.filter(|r| r.status == TestStatus::Failed) {
        let text = result.message.clone().unwrap_or_else(|| "Failed".into());
        rows = rows.child(failure_row(
            n + 1,
            depth + 1,
            text,
            result.location.clone(),
            module,
            line.unwrap_or(1),
            cx,
        ));
        used += 1;
    }
    (rows, used)
}

fn failure_row(
    n: usize,
    depth: usize,
    message: String,
    at: Option<test_runner::Location>,
    module: Ref,
    fallback_line: u32,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let (script, line) = match &at {
        Some(at) => (at.script, at.line),
        None => (module, fallback_line),
    };
    let place = at.map(|at| format!("at {}:{}", at.name, at.line));
    v_flex()
        .id(("tests-failure", n))
        .w_full()
        .pl(indent(depth))
        .pr_2()
        .pb_1()
        .text_size(tokens::text_xs())
        .text_color(tokens::text_error())
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.reveal_spec_line(script, line, window, cx);
        }))
        .child(message)
        .children(place)
}

#[allow(clippy::too_many_arguments)]
fn row(
    n: usize,
    depth: usize,
    chevron: Option<IconName>,
    status: Option<TestStatus>,
    name: String,
    millis: Option<u128>,
    busy: bool,
    selected: bool,
    cx: &mut Context<Shell>,
    on_click: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
    on_run: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> impl IntoElement {
    let (icon, color) = match status {
        Some(TestStatus::Passed) => (IconName::CircleCheck, cx.theme().success),
        Some(TestStatus::Failed) => (IconName::CircleX, cx.theme().danger),
        Some(TestStatus::Skipped) | None => (IconName::Minus, cx.theme().muted_foreground),
    };
    h_flex()
        .id(("tests-row", n))
        .w_full()
        .gap_2()
        .pl(indent(depth))
        .pr_2()
        .py_0p5()
        .text_size(tokens::text_xs())
        .when(selected, |this| this.bg(tokens::accent_soft()))
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .on_click(cx.listener(move |shell, _, window, cx| on_click(shell, window, cx)))
        .children(chevron.map(|c| Icon::new(c).small().text_color(tokens::text3())))
        .child(Icon::new(icon).small().text_color(color))
        .child(div().flex_1().text_color(tokens::text()).child(name))
        .children(millis.map(|ms| {
            div()
                .flex_none()
                .text_color(tokens::text3())
                .child(format!("{ms} ms"))
        }))
        .when(!busy, |this| {
            this.child(
                div()
                    .id(("tests-run", n))
                    .flex_none()
                    .rounded_sm()
                    .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        cx.stop_propagation();
                        on_run(shell, cx);
                    }))
                    .child(
                        Icon::new(IconName::Play)
                            .small()
                            .text_color(tokens::text2()),
                    ),
            )
        })
}

/// The selected row's title and what the pane shows for it: a test's
/// failure and output, or a file's whole output.
fn pane_contents(
    dock: &TestsDock,
    (module, path): &Key,
) -> Option<(String, Option<String>, Output)> {
    let file = dock.plans.iter().find(|p| p.file.module == *module)?;
    if path.is_empty() {
        let failure = file.error.clone().or_else(|| {
            dock.results
                .get(&(*module, Vec::new()))
                .and_then(|r| r.message.clone())
        });
        let output = dock.outputs.get(module).cloned().unwrap_or_default();
        return Some((file.file.name.clone(), failure, output));
    }
    let title = path.join(" › ");
    match dock.results.get(&(*module, path.clone())) {
        Some(result) => {
            let failure = result.message.clone().map(|message| match &result.stack {
                Some(stack) if !stack.is_empty() => format!("{message}\n{stack}"),
                _ => message,
            });
            Some((title, failure, result.output.clone()))
        }
        None => Some((title, None, Output::default())),
    }
}

impl Shell {
    fn tests_output_pane(&self) -> Option<impl IntoElement> {
        let dock = &self.tests;
        let (title, failure, output) = pane_contents(dock, dock.selected.as_ref()?)?;
        let empty = failure.is_none() && output.is_empty();
        let lines =
            output
                .lines()
                .iter()
                .zip(output.kinds())
                .enumerate()
                .map(|(i, (text, kind))| {
                    let color = match kind {
                        LineKind::Print => tokens::text(),
                        LineKind::Warn => tokens::warning(),
                        LineKind::Error => tokens::text_error(),
                    };
                    div()
                        .id(("tests-output-line", i))
                        .text_color(color)
                        .child(text.clone())
                });
        Some(
            v_flex()
                .id("tests-output")
                .w_full()
                .h(px(160.))
                .flex_none()
                .border_t_1()
                .border_color(tokens::border())
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text2())
                        .child(format!("Output: {title}")),
                )
                .child(
                    div()
                        .id("tests-output-lines")
                        .flex_1()
                        .overflow_y_scroll()
                        .track_scroll(&dock.output_scroll)
                        .px_2()
                        .pb_1()
                        .pr(chrome::SCROLLBAR_GUTTER)
                        .font_family(tokens::font_mono())
                        .text_size(tokens::text_xs())
                        .children(
                            failure.map(|text| {
                                div().text_color(tokens::text_error()).pb_1().child(text)
                            }),
                        )
                        .children(lines)
                        .when(empty, |this| {
                            this.text_color(tokens::text3()).child(match dock.running {
                                true => "Running…",
                                false => "Nothing printed",
                            })
                        })
                        .scrollbar_y(&dock.output_scroll),
                ),
        )
    }
}

fn indent(depth: usize) -> Pixels {
    px(8. + 14. * depth as f32)
}

/// The result for everything at or under `path` in one file: a failure
/// anywhere makes it failed, otherwise a pass beats a skip.
fn status_of(
    results: &HashMap<Key, TestResult>,
    module: Ref,
    path: &[String],
) -> Option<TestStatus> {
    let mut found = None;
    for ((m, p), result) in results {
        if *m != module || !p.starts_with(path) || p.is_empty() && !path.is_empty() {
            continue;
        }
        match result.status {
            TestStatus::Failed => return Some(TestStatus::Failed),
            TestStatus::Passed => found = Some(TestStatus::Passed),
            TestStatus::Skipped => found = found.or(Some(TestStatus::Skipped)),
        }
    }
    found
}

/// What the header says: that it is running, that there is nothing to run, or
/// the tally of the last results.
fn summary(
    running: bool,
    no_specs: bool,
    (passed, failed, skipped): (usize, usize, usize),
) -> String {
    if running {
        "Running…".into()
    } else if no_specs {
        "No specs: name a ModuleScript *.spec or *.test".into()
    } else {
        format!("{passed} passed, {failed} failed, {skipped} skipped")
    }
}

#[cfg(test)]
mod tests {
    use super::summary;

    #[test]
    fn the_summary_says_what_the_dock_is_doing_or_found() {
        assert_eq!(summary(true, false, (1, 1, 1)), "Running…");
        assert_eq!(
            summary(false, true, (0, 0, 0)),
            "No specs: name a ModuleScript *.spec or *.test"
        );
        assert_eq!(
            summary(false, false, (3, 1, 2)),
            "3 passed, 1 failed, 2 skipped"
        );
    }
}
