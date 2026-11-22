//! The MicroProfiler dock (Ctrl+F6, as in Studio): while Local Run runs,
//! a bar per frame split into script, physics, replication, other step work
//! and the editor's render, against the 60 Hz budget; the picked frame's
//! `debug.profilebegin` scopes laid out on its timeline; the ScriptProfiler
//! (`ScriptProfilerService`'s sampler, 1 or 10 KHz) as a callgraph or a
//! flat function list read back from its Developer Console JSON; and each
//! peer's Lua heap, the Developer Console's Memory figures.

mod chart;
mod report;
mod scripts;

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_sim::profile::FrameProfile;
use rbx_sim::Peer;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::Shell;
use crate::launcher::ui;
use crate::tokens;
use chart::profiler_frame;
use report::{report, Report};

/// Studio's key for the MicroProfiler, as a keymap action.
#[derive(Clone, PartialEq, serde::Deserialize, Action)]
#[action(namespace = shell, no_json)]
pub(crate) struct OpenMicroProfiler;

/// Frames the chart shows, the newest on the right.
const SHOWN: usize = 240;
/// One 60 Hz frame, the chart's guide line.
const BUDGET_MS: f64 = 1000.0 / 60.0;
const CHART_H: f32 = 96.0;
/// The chart's scale never goes under two budgets, so a calm run doesn't
/// fill it.
const MIN_SCALE_MS: f64 = BUDGET_MS * 2.0;
/// How often a running ScriptProfiler session is read back (the Developer
/// Console's Live polling: every second).
const POLL: Duration = Duration::from_secs(1);
const ROWS: usize = 200;

pub(crate) struct ProfilerDock {
    /// Frames held still for reading (Hold), else the live ones are shown.
    held: Option<Vec<FrameProfile>>,
    /// The frame picked on the chart, by number; the newest when `None`.
    picked: Option<u64>,
    /// Whose scripts the ScriptProfiler samples.
    peer: Peer,
    frequency: u32,
    /// Callgraph (by frame task) or a flat function list.
    flat: bool,
    report: Option<Report>,
    polled: Option<Instant>,
    chart: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl Default for ProfilerDock {
    fn default() -> Self {
        ProfilerDock {
            held: None,
            picked: None,
            peer: Peer::Server,
            frequency: rbx_lua::profiler::DEFAULT_FREQUENCY,
            flat: false,
            report: None,
            polled: None,
            chart: Rc::new(Cell::new(None)),
        }
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// A frame's five parts, in the order they stack: name, ms, colour.
type Parts = [(&'static str, f64, Rgba); 5];

/// The five parts of a frame, in the order they stack, with their colour.
fn parts(frame: &FrameProfile) -> Parts {
    [
        ("Script", ms(frame.script), ui::accent()),
        ("Physics", ms(frame.physics), tokens::tool_scale()),
        ("Replication", ms(frame.replication), tokens::warning()),
        ("Other", ms(frame.other), tokens::text3()),
        ("Render", ms(frame.render), tokens::tool_rotate()),
    ]
}

fn peer_label(peer: Peer) -> String {
    match peer {
        Peer::Server => "Server".into(),
        Peer::Client(i) => format!("Client {}", i + 1),
    }
}

/// The resident memory of the editor process, in MB.
fn resident_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|v| v.split_whitespace().next()?.parse::<f64>().ok())
        })
        .map_or(0.0, |kb| kb / 1024.0)
}

fn small(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .text_size(tokens::text_xs())
        .text_color(tokens::text2())
        .child(text.into())
}

fn heading(text: &'static str) -> Div {
    div()
        .px(px(8.))
        .pt(px(6.))
        .text_size(tokens::text_xs())
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens::text3())
        .child(text)
}

fn toggle(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    ui::button(
        id,
        label,
        if on {
            ui::Weight::Primary
        } else {
            ui::Weight::Ghost
        },
        true,
    )
}

impl Shell {
    pub(in crate::shell) fn open_micro_profiler(&mut self, cx: &mut Context<Self>) {
        self.set_panel_open(Panel::MicroProfiler, true, cx);
    }

    /// The frames on show: held ones, else the session's newest.
    fn profiler_frames(&self) -> Vec<FrameProfile> {
        if let Some(held) = &self.profiler.held {
            return held.clone();
        }
        self.local_run_session()
            .map(|s| {
                let frames = s.frames();
                frames
                    .iter()
                    .skip(frames.len().saturating_sub(SHOWN))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn profiler_poll(&mut self) {
        let peer = self.profiler.peer;
        let due = self.profiler.polled.is_none_or(|t| t.elapsed() >= POLL);
        let Some(sim) = self.local_run_session().and_then(|s| s.sim(peer)) else {
            return;
        };
        let profiler = sim.profiler();
        let profiler = profiler.borrow();
        if profiler.profiling() && due {
            self.profiler.report = report(&profiler.data());
            self.profiler.polled = Some(Instant::now());
        }
    }

    fn profiler_set_running(&mut self, on: bool, cx: &mut Context<Self>) {
        let (peer, frequency) = (self.profiler.peer, self.profiler.frequency);
        let Some(sim) = self.local_run_session().and_then(|s| s.sim(peer)) else {
            return;
        };
        let profiler = sim.profiler();
        let mut profiler = profiler.borrow_mut();
        if on {
            profiler.start(frequency);
            self.profiler.polled = None;
        } else {
            profiler.stop();
            self.profiler.report = report(&profiler.data());
        }
        cx.notify();
    }

    fn profiler_running(&self) -> bool {
        self.local_run_session()
            .and_then(|s| s.sim(self.profiler.peer))
            .is_some_and(|sim| sim.profiler().borrow().profiling())
    }

    pub(in crate::shell) fn micro_profiler_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::MicroProfilerOverflow,
            chrome::dock_options_trigger(
                "micro-profiler-overflow",
                IconName::Ellipsis,
                16.,
                "MicroProfiler options",
            ),
            self.move_items(Panel::MicroProfiler),
            cx,
        );
        if self.local_run_session().is_none() {
            self.profiler.held = None;
            let empty = div()
                .p(px(12.))
                .text_size(tokens::text_sm())
                .text_color(tokens::text2())
                .child(
                    "Start Run (local) or Play (local) to profile it: every frame's script, \
                     physics, replication and render time, the debug.profilebegin scopes \
                     scripts open, the ScriptProfiler and each peer's Lua memory.",
                );
            return (
                Some(overflow.into_any_element()),
                Some(empty.into_any_element()),
            );
        }
        self.profiler_poll();
        let frames = self.profiler_frames();
        let picked = self
            .profiler
            .picked
            .and_then(|n| frames.iter().find(|f| f.frame == n))
            .or(frames.last())
            .cloned();
        let body = v_flex()
            .id("micro-profiler-dock")
            .size_full()
            .gap_1()
            .pb_2()
            .overflow_y_scroll()
            .child(self.profiler_toolbar(cx))
            .child(self.profiler_legend(&frames))
            .child(self.profiler_chart(&frames, picked.as_ref().map(|f| f.frame), cx))
            .when_some(picked, |this, frame| {
                this.child(heading("FRAME")).child(profiler_frame(&frame))
            })
            .child(heading("SCRIPT PROFILER"))
            .child(self.profiler_scripts(cx))
            .child(heading("MEMORY"))
            .child(self.profiler_memory());
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    fn profiler_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let held = self.profiler.held.is_some();
        let running = self.profiler_running();
        let peers: Vec<Peer> = self
            .local_run_session()
            .map(|s| {
                std::iter::once(Peer::Server)
                    .chain((0..s.clients.len()).map(Peer::Client))
                    .collect()
            })
            .unwrap_or_default();
        h_flex()
            .px(px(8.))
            .pt(px(6.))
            .gap(px(6.))
            .items_center()
            .flex_wrap()
            .child(
                toggle("profiler-hold", if held { "Resume" } else { "Hold" }, held).on_click(
                    cx.listener(|shell, _, _, cx| {
                        shell.profiler.held = match shell.profiler.held {
                            Some(_) => None,
                            None => Some(shell.profiler_frames()),
                        };
                        cx.notify();
                    }),
                ),
            )
            .child(div().w(px(1.)).h(px(16.)).bg(tokens::border()))
            .children(peers.into_iter().map(|peer| {
                let on = self.profiler.peer == peer;
                toggle(
                    SharedString::from(format!("profiler-peer-{}", peer_label(peer))),
                    peer_label(peer),
                    on,
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    if shell.profiler.peer != peer {
                        shell.profiler_set_running(false, cx);
                        shell.profiler.peer = peer;
                        shell.profiler.report = None;
                    }
                    cx.notify();
                }))
            }))
            .child(div().flex_1())
            .children([1000u32, 10_000].map(|hz| {
                toggle(
                    SharedString::from(format!("profiler-hz-{hz}")),
                    format!("{} KHz", hz / 1000),
                    self.profiler.frequency == hz,
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.profiler.frequency = hz;
                    cx.notify();
                }))
            }))
            .child(
                ui::button(
                    "profiler-start",
                    if running { "Stop" } else { "Start" },
                    if running {
                        ui::Weight::Danger
                    } else {
                        ui::Weight::Primary
                    },
                    true,
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.profiler_set_running(!running, cx);
                })),
            )
    }

    fn profiler_memory(&self) -> impl IntoElement {
        let Some(session) = self.local_run_session() else {
            return v_flex();
        };
        let peers =
            std::iter::once(Peer::Server).chain((0..session.clients.len()).map(Peer::Client));
        let rows = peers.filter_map(|peer| {
            let sim = session.sim(peer)?;
            let heap = sim.heap_bytes() as f64 / (1024.0 * 1024.0);
            let category = sim
                .profiler()
                .borrow()
                .memory_category()
                .map(|c| format!("  ·  category {c}"))
                .unwrap_or_default();
            Some(small(format!(
                "{}  LuaHeap {heap:.2} MB{category}",
                peer_label(peer)
            )))
        });
        v_flex()
            .px(px(8.))
            .gap(px(2.))
            .children(rows)
            .child(small(format!("Editor process {:.0} MB", resident_mb())))
    }
}
