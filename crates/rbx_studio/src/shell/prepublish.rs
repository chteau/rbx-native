//! The Pre-publish dock: `rbx_audit::prepublish::check` over the whole
//! place (the Security Audit's findings, broken asset ids, deprecated API,
//! parts that fall at start, DataStore calls outside `pcall`, heavy
//! geometry, budgets), grouped by rule. Clicking a line finding opens the
//! script at that line; any other selects the instance.
//!
//! Save/Publish to Roblox runs the same check first (`roblox_publish`'s
//! `Dialog::Check`), skippable, unless "Don't ask again for this place" was
//! ticked; the dock's "Ask before publishing" box turns it back on.
//!
//! Like the Security Audit, the check is cached against the DOM's mutation
//! count and runs off the UI thread on a slim copy once edits are quiet.
//! Asset ids are asked about once per session (read-only GETs to Roblox's
//! asset delivery, no key); after a run that got no answer the automatic
//! re-runs stop asking until Check now, so an offline editor doesn't keep
//! waiting on the network.
//!
//! `RBX_STUDIO_PREPUBLISH=1` opens the dock at startup, and
//! `RBX_STUDIO_PREPUBLISH=offline` also skips the asset service: screenshot
//! aids like the other `RBX_STUDIO_*` variables.

use std::collections::HashMap;
use std::time::Duration;

use crate::shell::chrome::ScrollbarY as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_assets::{AssetCache, AssetRef};
use rbx_audit::prepublish::{self, Inputs, Presence};
use rbx_audit::{Check, Finding, Severity};

use crate::tokens;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::security_audit::message;
use super::Shell;

/// Read once at startup; documented in this module's doc comment.
const PREPUBLISH_VARIABLE: &str = "RBX_STUDIO_PREPUBLISH";

/// How long the DOM has to stay unchanged before an edit re-checks it.
const QUIET: Duration = Duration::from_millis(300);

/// Each rule as the dock's Rules list explains it; `docs/prepublish-check.md`
/// says the same at more length.
pub(super) fn rules() -> [(Check, String); 9] {
    use prepublish::{INSTANCE_BUDGET, MESH_TRIANGLE_LIMIT, PART_BUDGET, UNION_TRIANGLES};
    [
        (
            Check::BrokenAsset,
            "Content ids that aren't `rbxassetid://` references, that Roblox says don't exist or refuses, and empty ids a MeshPart, FileMesh, Animation, Sound or Decal needs.".into(),
        ),
        (
            Check::AssetsNotChecked,
            "Ids the asset service couldn't be asked about (offline). Read-only requests, once per id per session.".into(),
        ),
        (
            Check::DeprecatedApi,
            "`wait`, `spawn`, `delay`, `Instance.new(class, parent)`, `:connect`, `game.Workspace`, and methods the API dump tags Deprecated.".into(),
        ),
        (
            Check::UnanchoredParts,
            "Unanchored parts in the Workspace with no weld or constraint to an anchored part, outside characters and tools, counted per model.".into(),
        ),
        (
            Check::DataStoreWithoutPcall,
            "DataStore and MemoryStore calls not inside `pcall`, `xpcall` or a Promise.".into(),
        ),
        (
            Check::HeavyGeometry,
            format!("MeshParts over Roblox's {MESH_TRIANGLE_LIMIT}-triangle limit (error), unions over {UNION_TRIANGLES} triangles."),
        ),
        (
            Check::PlaceBudget,
            format!("More than {PART_BUDGET} parts or {INSTANCE_BUDGET} instances in the place."),
        ),
        (
            Check::StreamingIndex,
            "With StreamingEnabled, client code reaching a part with `.` instead of `WaitForChild`.".into(),
        ),
        (
            Check::UnvalidatedArgument,
            "Everything the Security Audit finds is listed too, under its own rules.".into(),
        ),
    ]
}

/// The last check and the DOM state it was taken at.
#[derive(Default)]
pub(super) struct PrePublishState {
    /// Grouped by rule, worst first within each.
    pub(super) findings: Vec<Finding>,
    pub(super) at: Option<u64>,
    /// The DOM state a check is queued or running for; dropping the task
    /// cancels it.
    pending: Option<(u64, Task<()>)>,
    /// The asset service's answers this session.
    presence: HashMap<u64, Presence>,
    /// The last run asked and got no answer: automatic re-runs don't ask.
    offline: bool,
    scroll: ScrollHandle,
    rules_open: bool,
    /// The linked place, read when `roblox.changes` was the first number.
    link: Option<(u64, Option<u64>)>,
}

impl Shell {
    /// `RBX_STUDIO_PREPUBLISH`: see this module's doc comment.
    pub(super) fn apply_debug_prepublish(&mut self, cx: &mut Context<Self>) {
        match std::env::var(PREPUBLISH_VARIABLE).as_deref() {
            Ok("offline") => {
                self.prepublish.offline = true;
                self.set_panel_open(Panel::PrePublish, true, cx);
            }
            Ok(_) => self.set_panel_open(Panel::PrePublish, true, cx),
            Err(_) => {}
        }
    }

    /// Whether Save/Publish to Roblox runs the check first for `place`.
    pub(super) fn prepublish_asks(&self, place: u64) -> bool {
        !self.prepublish_skip.contains(&place)
    }

    pub(super) fn set_prepublish_asks(&mut self, place: u64, ask: bool, cx: &mut Context<Self>) {
        self.prepublish_skip.retain(|p| *p != place);
        if !ask {
            self.prepublish_skip.push(place);
        }
        self.save_settings();
        cx.notify();
    }

    /// The place this file publishes to, read again only after a relink.
    fn linked_place(&mut self) -> Option<u64> {
        let changes = self.roblox.changes;
        match self.prepublish.link {
            Some((at, place)) if at == changes => place,
            _ => {
                let place = crate::home::link_of(&self.path).map(|link| link.place_id);
                self.prepublish.link = Some((changes, place));
                place
            }
        }
    }

    pub(super) fn prepublish_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        self.queue_prepublish(cx);

        let overflow = menu::dropdown(
            self,
            MenuId::PrePublishOverflow,
            chrome::dock_options_trigger(
                "prepublish-overflow",
                IconName::Ellipsis,
                16.,
                "Pre-publish dock options",
            ),
            self.move_items(Panel::PrePublish),
            cx,
        );

        let state = &self.prepublish;
        let status = match state.at {
            None => "Checking\u{2026}".to_string(),
            Some(_) => summary(&state.findings),
        };
        let ask = self
            .linked_place()
            .map(|place| (place, self.prepublish_asks(place)));
        let rules_open = self.prepublish.rules_open;
        let header =
            h_flex()
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
                        .min_w_0()
                        .truncate()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text2())
                        .child(status),
                )
                .children(ask.map(|(place, on)| {
                    h_flex()
                        .id("prepublish-ask")
                        .gap_1p5()
                        .px_1()
                        .rounded_sm()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text())
                        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.set_prepublish_asks(place, !on, cx)
                        }))
                        .child(checkbox(on))
                        .child("Ask before publishing")
                }))
                .child(header_button(
                    "prepublish-rules",
                    if rules_open { "Hide rules" } else { "Rules" },
                    cx.listener(|shell, _, _, cx| {
                        shell.prepublish.rules_open = !shell.prepublish.rules_open;
                        cx.notify();
                    }),
                ))
                .child(header_button(
                    "prepublish-rerun",
                    "Check now",
                    cx.listener(|shell, _, _, cx| shell.recheck_prepublish(cx)),
                ));

        let mut list = v_flex().w_full();
        if rules_open {
            list = list.child(rules_list());
        }
        let findings = &self.prepublish.findings;
        let mut index = 0;
        while index < findings.len() {
            let check = findings[index].check;
            let run = findings[index..]
                .iter()
                .take_while(|finding| finding.check == check)
                .count();
            list = list.child(rule_row(check, run));
            for (offset, finding) in findings.iter().enumerate().skip(index).take(run) {
                list = list.child(finding_row(offset, finding, cx));
            }
            index += run;
        }

        let body = v_flex().size_full().child(header).child(
            div()
                .id("prepublish-list")
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(&self.prepublish.scroll)
                .pr(chrome::SCROLLBAR_GUTTER)
                .child(list)
                .scrollbar_y(&self.prepublish.scroll),
        );
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    /// Runs the check for the upload dialog, asking the asset service
    /// again; a result already up to date lands at once.
    pub(super) fn check_before_upload(&mut self, cx: &mut Context<Self>) {
        self.prepublish.offline = false;
        self.queue_prepublish(cx);
        if self.prepublish.at == Some(self.edit_dom().mutations()) {
            self.prepublish_landed(cx);
        }
    }

    /// Check now: drops the last result and asks the asset service again.
    pub(super) fn recheck_prepublish(&mut self, cx: &mut Context<Self>) {
        self.prepublish.at = None;
        self.prepublish.pending = None;
        self.prepublish.offline = false;
        self.queue_prepublish(cx);
        cx.notify();
    }

    /// Checks the place in the background when it changed since the last
    /// check, after [`QUIET`] unless there is nothing to show yet.
    pub(super) fn queue_prepublish(&mut self, cx: &mut Context<Self>) {
        let stamp = self.edit_dom().mutations();
        let state = &self.prepublish;
        if state.at == Some(stamp) || state.pending.as_ref().is_some_and(|p| p.0 == stamp) {
            return;
        }
        let wait = state.at.is_some();
        let ask = !state.offline;
        let known = state.presence.clone();
        let task = cx.spawn(async move |shell, cx| {
            if wait {
                cx.background_executor().timer(QUIET).await;
            }
            // Edited again while waiting: the next render queues that one.
            let Ok(Some(copy)) = shell.update(cx, |shell, _| {
                (shell.edit_dom().mutations() == stamp)
                    .then(|| prepublish::slim_copy(shell.edit_dom()))
            }) else {
                return;
            };
            let (findings, answers, reached) = cx
                .background_spawn(async move { run(&copy, known, ask) })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                let state = &mut shell.prepublish;
                state.presence.extend(answers);
                state.offline = !reached;
                state.findings = findings;
                state.at = Some(stamp);
                state.pending = None;
                shell.prepublish_landed(cx);
                cx.notify();
            });
        });
        self.prepublish.pending = Some((stamp, task));
    }
}

/// One check, blocking: the findings grouped by rule, the new answers, and
/// whether the asset service answered (or there was nothing to ask).
fn run(
    dom: &rbx_dom::WeakDom,
    mut known: HashMap<u64, Presence>,
    ask: bool,
) -> (Vec<Finding>, HashMap<u64, Presence>, bool) {
    let unknown: Vec<u64> = prepublish::asset_ids(dom)
        .into_iter()
        .filter(|id| !known.contains_key(id))
        .collect();
    let answers = if ask && !unknown.is_empty() {
        rbx_cloud::Client::new(None).asset_presences(&unknown)
    } else {
        Vec::new()
    };
    let reached = unknown.is_empty() || answers.iter().any(Option::is_some);
    let fresh: HashMap<u64, Presence> = unknown
        .iter()
        .zip(answers)
        .filter_map(|(id, answer)| Some((*id, presence(answer?))))
        .collect();
    known.extend(fresh.iter().map(|(id, p)| (*id, *p)));
    let cache = rbx_assets::cache_root()
        .map(|root| root.join("assets"))
        .filter(|dir| dir.is_dir())
        .and_then(|dir| AssetCache::new(Some(dir)).ok());
    let mut fetch = |asset: &AssetRef| match (asset, &cache) {
        (AssetRef::Id(id), Some(cache)) => cache.get_id(*id),
        (AssetRef::Native(path), Some(cache)) => cache.get_native(path),
        _ => None,
    };
    let offline = !reached && known.is_empty();
    let mut findings = prepublish::check(
        dom,
        Inputs {
            presence: (!offline).then_some(&known),
            fetch: &mut fetch,
        },
    );
    // One group per rule; the stable sort keeps the worst first in each.
    findings.sort_by_key(|finding| finding.check);
    (findings, fresh, reached)
}

fn presence(answer: rbx_cloud::AssetPresence) -> Presence {
    match answer {
        rbx_cloud::AssetPresence::Exists => Presence::Exists,
        rbx_cloud::AssetPresence::Missing => Presence::Missing,
        rbx_cloud::AssetPresence::Restricted => Presence::Restricted,
    }
}

/// What a severity reads as here: a High finding is an error to fix before
/// publishing, the rest warnings and notes.
pub(super) fn severity_word(severity: Severity, count: usize) -> String {
    let word = match (severity, count) {
        (Severity::High, 1) => "error",
        (Severity::High, _) => "errors",
        (Severity::Medium, 1) => "warning",
        (Severity::Medium, _) => "warnings",
        (Severity::Low, _) => "info",
    };
    format!("{count} {word}")
}

/// The counts that are not zero, or that all is well.
pub(super) fn summary(findings: &[Finding]) -> String {
    let parts: Vec<String> = [Severity::High, Severity::Medium, Severity::Low]
        .into_iter()
        .map(|severity| {
            (
                severity,
                findings.iter().filter(|f| f.severity == severity).count(),
            )
        })
        .filter(|(_, n)| *n > 0)
        .map(|(severity, n)| severity_word(severity, n))
        .collect();
    if parts.is_empty() {
        return "No problems found".into();
    }
    parts.join(" \u{b7} ")
}

pub(super) fn severity_icon(severity: Severity, cx: &App) -> Icon {
    let (icon, color) = match severity {
        Severity::High => (IconName::CircleX, cx.theme().danger),
        Severity::Medium => (IconName::TriangleAlert, cx.theme().warning),
        Severity::Low => (IconName::Info, cx.theme().muted_foreground),
    };
    Icon::new(icon).small().text_color(color)
}

pub(super) fn checkbox(on: bool) -> impl IntoElement {
    div()
        .size(px(12.))
        .flex_none()
        .rounded(px(3.))
        .border_1()
        .border_color(if on {
            tokens::check_on()
        } else {
            tokens::check_off_border()
        })
        .when(on, |this| this.bg(tokens::check_on()))
        .flex()
        .items_center()
        .justify_center()
        .when(on, |this| {
            this.text_color(tokens::text_full())
                .child(Icon::new(IconName::Check).xsmall())
        })
}

fn header_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .px_2()
        .rounded_sm()
        .text_size(tokens::text_xs())
        .text_color(tokens::text())
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .on_click(on_click)
        .child(label)
}

fn rules_list() -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_1()
        .px_2()
        .py_1p5()
        .border_b_1()
        .border_color(tokens::border())
        .bg(tokens::wash_faint())
        .text_size(tokens::text_xs())
        .children(rules().into_iter().map(|(check, text)| {
            let title = if check == Check::UnvalidatedArgument {
                "Security Audit"
            } else {
                check.title()
            };
            h_flex()
                .w_full()
                .items_start()
                .gap_2()
                .child(
                    div()
                        .w(px(168.))
                        .flex_none()
                        .text_color(tokens::text_strong())
                        .child(title),
                )
                .child(div().flex_1().min_w_0().child(message(&text)))
        }))
}

fn rule_row(check: Check, count: usize) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_2()
        .px_2()
        .pt_1()
        .text_size(tokens::text_xs())
        .child(div().text_color(tokens::text_strong()).child(check.title()))
        .child(div().text_color(tokens::text3()).child(count.to_string()))
}

fn finding_row(index: usize, finding: &Finding, cx: &mut Context<Shell>) -> impl IntoElement {
    let (instance, line) = (finding.instance, finding.line);
    h_flex()
        .id(("prepublish-finding", index))
        .w_full()
        .gap_2()
        // Indented under its rule's row.
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
        .child(severity_icon(finding.severity, cx))
        .child(
            div()
                .flex_none()
                .text_color(tokens::text_strong())
                .child(match line {
                    Some(line) => format!("{}:{line}", finding.path),
                    None => finding.path.clone(),
                }),
        )
        .child(div().flex_1().min_w_0().child(message(&finding.message)))
}

#[cfg(test)]
mod tests {
    use super::{rules, summary};
    use rbx_audit::{Check, Finding, Severity};

    fn finding(severity: Severity) -> Finding {
        Finding {
            check: Check::BrokenAsset,
            severity,
            instance: rbx_dom::Ref::new(0),
            path: String::new(),
            line: None,
            message: String::new(),
        }
    }

    #[test]
    fn the_summary_counts_errors_warnings_and_info() {
        assert_eq!(summary(&[]), "No problems found");
        let findings = [
            finding(Severity::High),
            finding(Severity::Medium),
            finding(Severity::Medium),
            finding(Severity::Low),
        ];
        assert_eq!(
            summary(&findings),
            "1 error \u{b7} 2 warnings \u{b7} 1 info"
        );
        assert_eq!(summary(&findings[1..2]), "1 warning");
    }

    #[test]
    fn every_prepublish_rule_is_explained() {
        let explained: Vec<Check> = rules().into_iter().map(|(check, _)| check).collect();
        for check in [
            Check::BrokenAsset,
            Check::AssetsNotChecked,
            Check::DeprecatedApi,
            Check::UnanchoredParts,
            Check::DataStoreWithoutPcall,
            Check::HeavyGeometry,
            Check::PlaceBudget,
            Check::StreamingIndex,
        ] {
            assert!(explained.contains(&check), "{}", check.id());
        }
    }
}
