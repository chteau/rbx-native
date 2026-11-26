//! The Load test dock (Local Run beta): bot players on a headless Local Run
//! server of the open place, with live charts of what the server goes
//! through, and the bot count at which it stops keeping 60 Hz
//! (`rbx_sim::loadtest`).
//!
//! The run is its own session on a background thread at the lowest CPU
//! priority, apart from any Play: the bots draw nothing and have no window
//! or tab, so all there is to see is here. It never touches real data:
//! its DataStores live in memory.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_sim::bots::Mix;
use rbx_sim::loadtest::{self, Config, LoadTest, Ramp, Report, Sample};

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::Shell;
use crate::command_bar::Feedback;
use crate::launcher::ui;
use crate::tokens;

/// Seconds the charts show, the newest on the right.
const SHOWN: usize = 120;
const CHART_H: f32 = 56.0;
const POLL: Duration = Duration::from_millis(250);
/// The ramps on offer: none, or (step, seconds).
const RAMPS: [Option<(usize, f64)>; 3] = [None, Some((1, 5.0)), Some((5, 10.0))];

enum Update {
    /// A second's sample, the bots by behaviour, the breaking point so far.
    Sample(Sample, Vec<(String, usize)>, Option<usize>),
    Done(Report),
    Failed(String),
}

struct Run {
    target: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    updates: mpsc::Receiver<Update>,
    _poll: Task<()>,
}

/// Closing the place or the editor ends the run too.
impl Drop for Run {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub(crate) struct LoadTestDock {
    run: Option<Run>,
    bots: usize,
    ramp: Option<(usize, f64)>,
    remotes: bool,
    seed: u64,
    samples: Vec<Sample>,
    behaviours: Vec<(String, usize)>,
    breaking: Option<usize>,
    report: Option<Report>,
    error: Option<String>,
    slider: Option<Entity<SliderState>>,
}

impl Default for LoadTestDock {
    fn default() -> Self {
        LoadTestDock {
            run: None,
            bots: 10,
            ramp: Some((5, 10.0)),
            remotes: false,
            seed: 1,
            samples: Vec::new(),
            behaviours: Vec::new(),
            breaking: None,
            report: None,
            error: None,
            slider: None,
        }
    }
}

/// The run's thread: the lowest priority, so the editor and the rest of
/// the machine come first.
fn lower_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: setpriority on the calling thread (Linux nice values are per
    // thread) with plain integers.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
    }
}

fn small(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .text_size(tokens::text_xs())
        .text_color(tokens::text2())
        .child(text.into())
}

fn toggle(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    let weight = if on {
        ui::Weight::Primary
    } else {
        ui::Weight::Ghost
    };
    ui::button(id, label, weight, true)
}

/// A chart of one second per bar: each series' values, its colour, and a
/// guide line (a budget) when given.
fn chart(
    title: String,
    series: Vec<(Vec<f64>, Rgba)>,
    guide: Option<f64>,
    floor: f64,
) -> impl IntoElement {
    let peak = series
        .iter()
        .flat_map(|(v, _)| v.iter().copied())
        .chain(guide.map(|g| g * 1.25))
        .fold(floor, f64::max);
    let guide_y = guide.map(|g| (1.0 - g / peak) as f32);
    v_flex()
        .w(relative(0.5))
        .px(px(8.))
        .pt(px(4.))
        .gap(px(2.))
        .child(small(title))
        .child(
            div()
                .h(px(CHART_H))
                .relative()
                .rounded(tokens::radius())
                .bg(tokens::field_select())
                .border_1()
                .border_color(tokens::border())
                .overflow_hidden()
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let w = f32::from(bounds.size.width);
                            let h = f32::from(bounds.size.height);
                            let slot = w / SHOWN as f32;
                            let lanes = series.len().max(1) as f32;
                            for (lane, (values, colour)) in series.iter().enumerate() {
                                let start = SHOWN.saturating_sub(values.len());
                                for (i, value) in values.iter().enumerate() {
                                    let bar = (*value / peak) as f32 * h;
                                    if bar <= 0.0 {
                                        continue;
                                    }
                                    let x = f32::from(bounds.origin.x)
                                        + (start + i) as f32 * slot
                                        + lane as f32 * slot / lanes;
                                    window.paint_quad(fill(
                                        Bounds {
                                            origin: point(px(x), bounds.origin.y + px(h - bar)),
                                            size: size(px((slot / lanes - 0.5).max(1.0)), px(bar)),
                                        },
                                        *colour,
                                    ));
                                }
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .when_some(guide_y, |this, y| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(relative(y))
                            .h(px(1.))
                            .bg(tokens::text_error()),
                    )
                })
                .child(
                    div()
                        .absolute()
                        .left(px(4.))
                        .top(px(2.))
                        .px(px(2.))
                        .rounded(tokens::radius())
                        .bg(tokens::field_select())
                        .child(small(format!("{peak:.0}"))),
                ),
        )
}

impl Shell {
    fn load_test_slider(&mut self, cx: &mut Context<Self>) -> Entity<SliderState> {
        if let Some(slider) = &self.load_test.slider {
            return slider.clone();
        }
        let slider = cx.new(|_| {
            SliderState::new()
                .min(1.)
                .max(loadtest::MAX_BOTS as f32)
                .step(1.)
                .default_value(self.load_test.bots as f32)
        });
        cx.subscribe(&slider, |shell, _, event: &SliderEvent, cx| {
            if let SliderEvent::Change(SliderValue::Single(value)) = event {
                shell.load_test.bots = value.round().max(1.0) as usize;
                if let Some(run) = &shell.load_test.run {
                    run.target.store(shell.load_test.bots, Ordering::Relaxed);
                }
                cx.notify();
            }
        })
        .detach();
        self.load_test.slider = Some(slider.clone());
        slider
    }

    fn load_test_start(&mut self, cx: &mut Context<Self>) {
        let dock = &mut self.load_test;
        let config = Config {
            bots: dock.bots,
            ramp: dock.ramp.map(|(step, every)| Ramp { step, every }),
            duration: None,
            seed: dock.seed,
            mix: Mix {
                remotes: if dock.remotes { 2 } else { 0 },
                ..Mix::default()
            },
            min_hz: loadtest::DEFAULT_MIN_HZ,
        };
        dock.samples.clear();
        dock.behaviours.clear();
        dock.breaking = None;
        dock.report = None;
        dock.error = None;
        let place = self.dom.clone();
        let name = self
            .path
            .file_name()
            .map_or("place".to_owned(), |n| n.to_string_lossy().into_owned());
        let target = Arc::new(AtomicUsize::new(config.bots));
        let stop = Arc::new(AtomicBool::new(false));
        let (send, updates) = mpsc::channel();
        let (thread_target, thread_stop) = (target.clone(), stop.clone());
        let started = std::thread::Builder::new()
            .name("load-test".into())
            .spawn(move || {
                lower_priority();
                let mut test = match LoadTest::new(place, &name, config, None) {
                    Ok(test) => test,
                    Err(why) => {
                        let _ = send.send(Update::Failed(why));
                        return;
                    }
                };
                test.session.set_fetcher(Arc::new(super::local_run::Fetcher(
                    rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config()),
                )));
                rbx_lua::server::set_text_measure(Arc::new(rbx_viewer::measure_text));
                let ran = test.run(|test, sampled| {
                    test.set_target(thread_target.load(Ordering::Relaxed));
                    if let Some(sample) = sampled.then(|| test.samples().last()).flatten() {
                        let mut counts: Vec<(String, usize)> = Vec::new();
                        for (_, behaviour) in test.bots.roster() {
                            match counts.iter_mut().find(|(k, _)| k == behaviour.name()) {
                                Some((_, n)) => *n += 1,
                                None => counts.push((behaviour.name().to_owned(), 1)),
                            }
                        }
                        let update = Update::Sample(sample.clone(), counts, test.breaking_point());
                        let _ = send.send(update);
                    }
                    !thread_stop.load(Ordering::Relaxed)
                });
                let _ = send.send(match ran {
                    Ok(()) => Update::Done(test.finish()),
                    Err(why) => Update::Failed(why),
                });
            });
        if let Err(why) = started {
            self.load_test.error = Some(why.to_string());
            return;
        }
        let poll = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let going = this.update(cx, |shell, cx| shell.load_test_poll(cx));
            if !matches!(going, Ok(true)) {
                break;
            }
        });
        self.load_test.run = Some(Run {
            target,
            stop,
            updates,
            _poll: poll,
        });
        cx.notify();
    }

    /// What the run sent since the last look; false once it ended.
    fn load_test_poll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(run) = &self.load_test.run else {
            return false;
        };
        let updates: Vec<Update> = run.updates.try_iter().collect();
        let mut going = true;
        let dock = &mut self.load_test;
        for update in updates {
            match update {
                Update::Sample(sample, behaviours, breaking) => {
                    dock.samples.push(sample);
                    dock.behaviours = behaviours;
                    dock.breaking = breaking;
                }
                Update::Done(report) => {
                    dock.breaking = report.breaking_point;
                    dock.report = Some(report);
                    going = false;
                }
                Update::Failed(why) => {
                    dock.error = Some(why);
                    going = false;
                }
            }
        }
        if !going {
            dock.run = None;
        }
        cx.notify();
        going
    }

    fn load_test_stop(&mut self) {
        if let Some(run) = &self.load_test.run {
            run.stop.store(true, Ordering::Relaxed);
        }
    }

    /// Export: the report as Markdown and JSON in the data directory.
    fn load_test_export(&mut self, cx: &mut Context<Self>) {
        let Some(report) = &self.load_test.report else {
            return;
        };
        let row = match export(report) {
            Ok(path) => Feedback::Info(format!(
                "Load test report saved to {} (and .json)",
                path.display()
            )),
            Err(why) => {
                Feedback::Error(format!("Couldn\u{2019}t save the load test report: {why}"))
            }
        };
        super::local_run::push_row(&mut self.output, row);
        cx.notify();
    }

    pub(in crate::shell) fn load_test_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::LoadTestOverflow,
            chrome::dock_options_trigger(
                "load-test-overflow",
                IconName::Ellipsis,
                16.,
                "Load test options",
            ),
            self.move_items(Panel::LoadTest),
            cx,
        );
        let slider = self.load_test_slider(cx);
        let dock = &self.load_test;
        let running = dock.run.is_some();
        let s = &dock.samples[dock.samples.len().saturating_sub(SHOWN)..];
        let series = |f: fn(&Sample) -> f64| s.iter().map(f).collect::<Vec<f64>>();
        let last = s.last();
        let players = last.map_or(0, |x| x.bots);
        let status = match (running, last) {
            (true, Some(x)) => format!(
                "Running · {} of {} bots · {:.0} s · {}",
                x.bots,
                dock.bots,
                x.t,
                dock.behaviours
                    .iter()
                    .map(|(k, n)| format!("{n} {k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            (true, None) => "Starting…".to_owned(),
            (false, Some(x)) => format!("Stopped after {:.0} s", x.t),
            (false, None) => "Bots join a headless Local Run server of this place, \
                              in memory only: no real data is read or written."
                .to_owned(),
        };
        let verdict = match dock.breaking {
            Some(n) => format!("Broke 60 Hz at {n} bots"),
            None if last.is_some() => format!(
                "Kept 60 Hz{} (up to {} bots)",
                if running { " so far" } else { "" },
                s.iter().map(|x| x.bots).max().unwrap_or(0)
            ),
            None => String::new(),
        };
        let warning = (dock.bots > loadtest::DEFAULT_CAP).then(|| {
            format!(
                "More than {} bots: each is a whole client on this machine, which may get sluggish.",
                loadtest::DEFAULT_CAP
            )
        });
        let toolbar = h_flex()
            .px(px(8.))
            .pt(px(6.))
            .gap(px(6.))
            .items_center()
            .flex_wrap()
            .child(
                ui::button(
                    "load-test-start",
                    if running { "Stop" } else { "Start" },
                    if running {
                        ui::Weight::Danger
                    } else {
                        ui::Weight::Primary
                    },
                    true,
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    if running {
                        shell.load_test_stop();
                    } else {
                        shell.load_test_start(cx);
                    }
                    cx.notify();
                })),
            )
            .child(small(format!("Bots {}", dock.bots)))
            .child(
                div()
                    .w(px(160.))
                    .child(super::rows::rail_slider(&slider, cx)),
            )
            .children(RAMPS.into_iter().enumerate().map(|(i, ramp)| {
                let label = match ramp {
                    None => "All at once".to_owned(),
                    Some((step, every)) => format!("+{step} / {every:.0} s"),
                };
                toggle(
                    SharedString::from(format!("load-test-ramp-{i}")),
                    label,
                    dock.ramp == ramp,
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.load_test.ramp = ramp;
                    cx.notify();
                }))
            }))
            .child(
                toggle("load-test-remotes", "Fire remotes", dock.remotes).on_click(cx.listener(
                    |shell, _, _, cx| {
                        shell.load_test.remotes = !shell.load_test.remotes;
                        cx.notify();
                    },
                )),
            )
            .child(div().flex_1())
            .child(small(format!("Seed {}", dock.seed)))
            .child(
                ui::button(
                    "load-test-export",
                    "Export report",
                    ui::Weight::Secondary,
                    true,
                )
                .when(dock.report.is_none(), |b| b.opacity(0.5))
                .on_click(cx.listener(|shell, _, _, cx| shell.load_test_export(cx))),
            );
        let budget = 1000.0 / rbx_sim::FRAME_HZ;
        let charts = h_flex()
            .flex_wrap()
            .child(chart(
                format!(
                    "Server step  {:.2} ms / frame, worst {:.1} (budget 16.67)",
                    last.map_or(0.0, |x| x.server_ms),
                    last.map_or(0.0, |x| x.server_ms_max)
                ),
                vec![(series(|x| x.server_ms), ui::accent())],
                Some(budget),
                budget,
            ))
            .child(chart(
                format!("Heartbeat  {:.1} Hz", last.map_or(0.0, |x| x.heartbeat_hz)),
                vec![(series(|x| x.heartbeat_hz), tokens::tool_scale())],
                Some(loadtest::DEFAULT_MIN_HZ),
                60.0,
            ))
            .child(chart(
                format!("Bots  {players}"),
                vec![(series(|x| x.bots as f64), tokens::tool_rotate())],
                None,
                5.0,
            ))
            .child(chart(
                format!("Luau heap  {:.1} MB", last.map_or(0.0, |x| x.heap_mb)),
                vec![(series(|x| x.heap_mb), ui::accent())],
                None,
                1.0,
            ))
            .child(chart(
                format!("Instances  {}", last.map_or(0, |x| x.instances)),
                vec![(series(|x| x.instances as f64), tokens::text3())],
                None,
                10.0,
            ))
            .child(chart(
                format!(
                    "Physics parts  {} ({} assemblies)",
                    last.map_or(0, |x| x.parts),
                    last.map_or(0, |x| x.assemblies)
                ),
                vec![(series(|x| x.parts as f64), tokens::tool_scale())],
                None,
                10.0,
            ))
            .child(chart(
                format!(
                    "DataStore  {:.0} / min (GetAsync budget {:.0})",
                    last.map_or(0.0, |x| x.datastore_per_min),
                    loadtest::datastore_budget("GetAsync", players).unwrap_or(0.0)
                ),
                vec![(series(|x| x.datastore_per_min), tokens::warning())],
                loadtest::datastore_budget("GetAsync", players),
                10.0,
            ))
            .child(chart(
                format!(
                    "MemoryStore  {:.0} / min (budget {:.0})",
                    last.map_or(0.0, |x| x.memorystore_per_min),
                    loadtest::memorystore_budget(players)
                ),
                vec![(series(|x| x.memorystore_per_min), tokens::warning())],
                Some(loadtest::memorystore_budget(players)),
                10.0,
            ))
            .child(chart(
                format!(
                    "Remotes  {:.0} in / {:.0} out per s",
                    last.map_or(0.0, |x| x.remotes_in),
                    last.map_or(0.0, |x| x.remotes_out)
                ),
                vec![
                    (series(|x| x.remotes_in), ui::accent()),
                    (series(|x| x.remotes_out), tokens::tool_rotate()),
                ],
                None,
                5.0,
            ))
            .child(chart(
                format!(
                    "Remote data  {:.1} KB in / {:.1} KB out per s",
                    last.map_or(0.0, |x| x.kb_in),
                    last.map_or(0.0, |x| x.kb_out)
                ),
                vec![
                    (series(|x| x.kb_in), ui::accent()),
                    (series(|x| x.kb_out), tokens::tool_rotate()),
                ],
                None,
                1.0,
            ));
        let per_bot = dock.report.as_ref().map(|r| {
            format!(
                "Per bot on this machine: {:.2} ms client step, {:.2} MB Luau heap, {:.1} MB memory",
                r.per_bot.client_ms, r.per_bot.client_heap_mb, r.per_bot.memory_mb
            )
        });
        let body = v_flex()
            .id("load-test-dock")
            .size_full()
            .gap_1()
            .pb_2()
            .overflow_y_scroll()
            .child(toolbar)
            .child(
                h_flex()
                    .px(px(8.))
                    .gap(px(12.))
                    .flex_wrap()
                    .child(small(status))
                    .child(
                        div()
                            .text_size(tokens::text_xs())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if dock.breaking.is_some() {
                                tokens::text_error()
                            } else {
                                tokens::text2()
                            })
                            .child(verdict),
                    ),
            )
            .when_some(warning, |this, w| {
                this.child(
                    div()
                        .px(px(8.))
                        .text_size(tokens::text_xs())
                        .text_color(tokens::warning())
                        .child(w),
                )
            })
            .when_some(dock.error.clone(), |this, e| {
                this.child(
                    div()
                        .px(px(8.))
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text_error())
                        .child(format!("The load test stopped: {e}")),
                )
            })
            .when_some(per_bot, |this, line| {
                this.child(div().px(px(8.)).child(small(line)))
            })
            .child(charts);
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }
}

fn export(report: &Report) -> Result<std::path::PathBuf, String> {
    let dir = rbx_sim::replay::default_dir()
        .and_then(|d| Some(d.parent()?.join("loadtests")))
        .ok_or("no data directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stem = report
        .place
        .trim_end_matches(".rbxl")
        .trim_end_matches(".rbxlx");
    let path = dir.join(format!("{stem}-{stamp}.md"));
    std::fs::write(&path, report.markdown()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&report.json()).map_err(|e| e.to_string())?;
    std::fs::write(path.with_extension("json"), json).map_err(|e| e.to_string())?;
    Ok(path)
}
