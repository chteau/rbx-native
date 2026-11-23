//! Test › Replay: every Local Run is recorded (`rbx_sim::replay`); Replay
//! plays the last one back frame for frame, with a transport under the
//! ribbon (play/pause, speed, step a frame, a rail to scrub), and says
//! where the replay first left the recording. Scrubbing back re-simulates
//! from the start: a Luau VM can't be snapshotted, so there are no
//! keyframes to jump to. Save recording keeps one in the user's data
//! directory for `examples/replay.rs` or a later session.

use std::path::Path;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::WeakDom;
use rbx_sim::replay::{Divergence, Playback, Recording};

use super::{changes_between, push_row, stash_edited, Running, Stash, TICK};
use crate::command_bar::Feedback;
use crate::history::{History, DEFAULT_CAP};
use crate::properties::Properties;
use crate::shell::{chrome, Shell};
use crate::tokens;

/// Frames a scrub runs per tick on its way to where it was dropped.
pub(super) const SEEK_FRAMES_PER_TICK: u64 = 120;

const SPEEDS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// A replay in progress.
pub(super) struct ReplayState {
    pub(super) playback: Playback,
    pub(super) speed: f64,
    /// Where it first left the recording.
    pub(super) divergence: Option<Divergence>,
    /// A scrub on its way to this frame.
    pub(super) seek: Option<u64>,
}

/// A recording's seed, different every run.
pub(super) fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

/// The session `recording` plays into and its playback, past its first
/// frame (the start and the joins), with the peer to show.
fn begin(recording: Recording) -> Result<(rbx_sim::Session, Playback, Option<Divergence>), String> {
    let (mut session, mut playback) = Playback::start(recording)?;
    let first = playback.step(&mut session).err();
    session.server.watch_view_changes();
    for client in &mut session.clients {
        client.sim.watch_view_changes();
    }
    Ok((session, playback, first))
}

fn shown(session: &rbx_sim::Session, view: rbx_sim::Peer) -> WeakDom {
    let mut dom = session
        .sim(view)
        .map(|s| s.dom().clone())
        .unwrap_or_default();
    dom.take_changes();
    dom
}

impl Shell {
    pub(crate) fn local_run_replaying(&self) -> bool {
        self.play
            .live
            .local
            .running
            .as_ref()
            .is_some_and(|run| run.replay.is_some())
    }

    /// The recording Replay and Save recording would take: the running
    /// run's, else the last one's.
    fn local_run_current_recording(&self) -> Option<Recording> {
        if !crate::beta::enabled(super::BETA_ID) {
            return None;
        }
        match &self.play.live.local.running {
            Some(run) => match &run.replay {
                Some(replay) => Some(replay.playback.recording().clone()),
                None => run.session.recording(),
            },
            None => self.play.live.local.recording.clone(),
        }
    }

    pub(crate) fn local_run_has_recording(&self) -> bool {
        match &self.play.live.local.running {
            Some(_) => true,
            None => self.play.live.local.recording.is_some(),
        }
    }

    /// Save recording: into the user's data directory, named after the
    /// place and the time.
    pub(crate) fn local_run_save_recording(&mut self, cx: &mut Context<Self>) {
        let Some(recording) = self.local_run_current_recording() else {
            return;
        };
        let row = match rbx_sim::replay::default_dir() {
            Some(dir) => {
                let stem = self
                    .path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("place");
                let path = dir.join(format!("{stem}-{}.rbxnrec", seed() / 1_000_000));
                match recording.save(&path) {
                    Ok(()) => Feedback::Info(format!(
                        "Recording saved ({} frames) to {}",
                        recording.frame_count(),
                        path.display()
                    )),
                    Err(why) => {
                        Feedback::Error(format!("Couldn\u{2019}t save the recording: {why}"))
                    }
                }
            }
            None => Feedback::Error("Couldn\u{2019}t save the recording: no data directory".into()),
        };
        push_row(&mut self.output, row);
        cx.notify();
    }

    /// Replay: the run in progress stops (keeping its recording) and its
    /// recording, or the last one, plays back.
    pub(crate) fn local_run_replay(&mut self, cx: &mut Context<Self>) {
        let Some(recording) = self.local_run_current_recording() else {
            return;
        };
        if self.local_run_active() {
            self.local_run_stop(cx);
        }
        self.local_run_replay_recording(recording, cx);
    }

    /// `RBX_STUDIO_LOCAL_RUN=replay:<file>`: plays a saved recording back
    /// at startup.
    pub(super) fn local_run_replay_file(&mut self, file: &Path, cx: &mut Context<Self>) {
        match Recording::load(file) {
            Ok(recording) => self.local_run_replay_recording(recording, cx),
            Err(why) => push_row(
                &mut self.output,
                Feedback::Error(format!("Couldn\u{2019}t open the recording: {why}")),
            ),
        }
    }

    fn local_run_replay_recording(&mut self, recording: Recording, cx: &mut Context<Self>) {
        if !crate::beta::enabled(super::BETA_ID) {
            return;
        }
        let frames = recording.frame_count();
        let engine = recording.engine.clone();
        let (session, playback, first) = match begin(recording) {
            Ok(begun) => begun,
            Err(why) => {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("Replay couldn\u{2019}t start: {why}")),
                );
                cx.notify();
                return;
            }
        };
        let view = if session.clients.is_empty() {
            rbx_sim::Peer::Server
        } else {
            rbx_sim::Peer::Client(0)
        };
        let shown = shown(&session, view);
        let history = std::mem::replace(&mut self.history, History::new(DEFAULT_CAP));
        let selection = self.selected_all().to_vec();
        self.deselect(cx);
        let back = changes_between(&self.dom, &shown);
        let dom = stash_edited(&mut self.dom, shown);
        let snapshots = self.dom.snapshot(&back);
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, back));
        let task = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            if this
                .update(cx, |shell, cx| shell.local_run_tick(cx))
                .is_err()
            {
                break;
            }
        });
        if let Some(divergence) = &first {
            push_row(
                &mut self.output,
                Feedback::Error(format!("Replay {divergence}")),
            );
        }
        let clock = session.time();
        self.play.live.local.running = Some(Running {
            session,
            view,
            stash: Stash {
                dom,
                history,
                selection,
            },
            paused: false,
            step_once: false,
            clock,
            last: Instant::now(),
            task: Some(task),
            playing: false,
            joining: false,
            sound: super::sound::Mixer::new(self.viewport.read(cx).core_gui.volume),
            prompts: std::collections::VecDeque::new(),
            editables: rbx_dom::editable::generation(),
            player: None,
            pad: None,
            replay: Some(ReplayState {
                playback,
                speed: 1.0,
                divergence: first,
                seek: None,
            }),
            windowed: false,
            windows: Vec::new(),
            test_window: None,
            audible: None,
            remotes: Default::default(),
        });
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        let note = if engine == rbx_sim::replay::ENGINE {
            String::new()
        } else {
            format!(
                " (recorded on engine {engine}, this is {})",
                rbx_sim::replay::ENGINE
            )
        };
        push_row(
            &mut self.output,
            Feedback::Info(format!(
                "Replaying the recording: {frames} frames, {:.1} s{note}",
                frames as f64 / super::FRAME_HZ
            )),
        );
        cx.notify();
    }

    /// Back to the recording's first frame, re-simulated: a scrub back
    /// starts here.
    pub(super) fn local_run_restart_replay(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let Some(replay) = run.replay.as_mut() else {
            return;
        };
        let (session, playback, first) = match begin(replay.playback.recording().clone()) {
            Ok(begun) => begun,
            Err(why) => {
                replay.seek = None;
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("Replay couldn\u{2019}t restart: {why}")),
                );
                return;
            }
        };
        let view = match run.view {
            rbx_sim::Peer::Client(i) if i < session.clients.len() => run.view,
            _ if !session.clients.is_empty() => rbx_sim::Peer::Client(0),
            _ => rbx_sim::Peer::Server,
        };
        let shown = shown(&session, view);
        replay.playback = playback;
        replay.divergence = first;
        run.session = session;
        run.view = view;
        run.clock = run.session.time();
        self.deselect(cx);
        let back = changes_between(&self.dom, &shown);
        self.dom = shown;
        let snapshots = self.dom.snapshot(&back);
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, back));
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
    }

    fn local_run_replay_speed(&mut self, cx: &mut Context<Self>) {
        if let Some(replay) = self
            .play
            .live
            .local
            .running
            .as_mut()
            .and_then(|r| r.replay.as_mut())
        {
            let at = SPEEDS.iter().position(|s| *s == replay.speed).unwrap_or(2);
            replay.speed = SPEEDS[(at + 1) % SPEEDS.len()];
            cx.notify();
        }
    }

    /// A scrub on the rail: `fraction` of the way through the recording.
    fn local_run_replay_seek(&mut self, fraction: f32, cx: &mut Context<Self>) {
        if let Some(replay) = self
            .play
            .live
            .local
            .running
            .as_mut()
            .and_then(|r| r.replay.as_mut())
        {
            let frames = replay.playback.frame_count();
            replay.seek = Some((f64::from(fraction) * frames as f64).round() as u64);
            self.local_run_tick(cx);
            cx.notify();
        }
    }

    fn replay_rail(&mut self, cx: &mut Context<Self>) -> Entity<SliderState> {
        if let Some(rail) = &self.play.live.local.rail {
            return rail.clone();
        }
        let rail = cx.new(|_| SliderState::new().min(0.).max(1.).step(0.0005));
        cx.subscribe(&rail, |shell, _, event: &SliderEvent, cx| {
            if let SliderEvent::Change(SliderValue::Single(value)) = event {
                shell.local_run_replay_seek(*value, cx);
            }
        })
        .detach();
        self.play.live.local.rail = Some(rail.clone());
        rail
    }

    /// The replay transport, under the ribbon while a replay runs.
    pub(crate) fn replay_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let run = self.play.live.local.running.as_ref()?;
        let replay = run.replay.as_ref()?;
        let frames = replay.playback.frame_count().max(1);
        let frame = run.session.server.frame().min(frames);
        let (paused, speed, seek) = (run.paused, replay.speed, replay.seek);
        let seeking = seek.is_some();
        let divergence = replay.divergence.clone();
        let rail = self.replay_rail(cx);
        // A scrub on its way shows where it's going.
        let at = seek.unwrap_or(frame).min(frames) as f32 / frames as f32;
        if rail.read(cx).value().end() != at {
            rail.update(cx, |rail, cx| rail.set_value(at, window, cx));
        }
        let tool = |id: &'static str, icon, label, enabled: bool| {
            chrome::icon_button(id, icon, label)
                .debug_selector(move || id.into())
                .when(!enabled, |this| this.opacity(0.4).cursor_not_allowed())
        };
        let status = match &divergence {
            Some(d) => {
                // The headline; the changes that differed are in the Output.
                let full = d.to_string();
                let text = match full.split_once("; in the replay") {
                    Some((head, _)) => format!("{head} (details in the Output)"),
                    None => full,
                };
                h_flex()
                    .id("replay-status")
                    .gap(px(5.))
                    .items_center()
                    .text_color(tokens::warning())
                    .child(Icon::new(IconName::TriangleAlert).size(tokens::text_sm()))
                    .child(format!("Diverged at frame {}", d.frame))
                    .tooltip(move |window, cx| {
                        crate::shell::tooltip::text(text.clone(), window, cx)
                    })
            }
            None => h_flex()
                .id("replay-status")
                .gap(px(5.))
                .items_center()
                .text_color(tokens::diff_add())
                .child(Icon::new(IconName::CircleCheck).size(tokens::text_sm()))
                .child("Matches the recording")
                .tooltip(|window, cx| {
                    crate::shell::tooltip::text(
                        "Every frame so far hashed the same as when it was recorded.",
                        window,
                        cx,
                    )
                }),
        };
        let readout = format!(
            "{frame} / {frames}  \u{b7}  {:.2} s",
            frame as f64 / super::FRAME_HZ
        );
        let speed_label = match speed {
            s if s < 1.0 => format!("{s}\u{d7}"),
            s => format!("{}\u{d7}", s as u32),
        };
        Some(
            h_flex()
                .id("replay-bar")
                .w_full()
                .h(tokens::dock_tabs_height())
                .flex_none()
                .items_center()
                .gap(px(4.))
                .px(px(10.))
                .bg(tokens::chrome())
                .border_b(px(1.))
                .border_color(tokens::border())
                .text_size(tokens::text_sm())
                .text_color(tokens::text_label())
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .pr(px(6.))
                        .child(Icon::new(IconName::Film).size(tokens::text_sm()))
                        .child(div().text_color(tokens::text_strong()).child("Replay")),
                )
                .child(
                    tool(
                        "replay-restart",
                        IconName::RotateCcw,
                        "Back to the start",
                        true,
                    )
                    .on_click(cx.listener(|shell, _, _, cx| shell.local_run_replay_seek(0., cx))),
                )
                .child(
                    tool(
                        "replay-play",
                        if paused {
                            IconName::Play
                        } else {
                            IconName::Pause
                        },
                        "Play / pause",
                        true,
                    )
                    .on_click(cx.listener(|shell, _, _, cx| shell.local_run_pause(cx))),
                )
                .child(
                    tool(
                        "replay-step",
                        IconName::ChevronRight,
                        "Step one frame",
                        paused,
                    )
                    .on_click(cx.listener(|shell, _, _, cx| shell.local_run_step_frame(cx))),
                )
                .child(
                    div()
                        .id("replay-speed")
                        .debug_selector(|| "replay-speed".into())
                        .min_w(px(40.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(tokens::radius())
                        .text_center()
                        .text_color(tokens::text_strong())
                        .hover(|this| this.bg(tokens::accent_soft()))
                        .cursor_pointer()
                        .child(speed_label)
                        .tooltip(|window, cx| {
                            crate::shell::tooltip::text("Playback speed", window, cx)
                        })
                        .on_click(cx.listener(|shell, _, _, cx| shell.local_run_replay_speed(cx))),
                )
                .child(
                    div()
                        .flex_1()
                        .mx(px(8.))
                        .when(seeking, |this| this.opacity(0.6))
                        .child(crate::shell::rows::rail_slider(&rail, cx)),
                )
                .child(
                    div()
                        .min_w(px(132.))
                        .text_color(tokens::text_muted())
                        .child(readout),
                )
                .child(status)
                .child(div().w(px(8.)))
                .child(
                    tool("replay-save", IconName::Save, "Save recording", true).on_click(
                        cx.listener(|shell, _, _, cx| shell.local_run_save_recording(cx)),
                    ),
                )
                .child(
                    tool("replay-exit", IconName::Close, "Leave the replay", true)
                        .on_click(cx.listener(|shell, _, _, cx| shell.local_run_stop(cx))),
                )
                .into_any_element(),
        )
    }
}
