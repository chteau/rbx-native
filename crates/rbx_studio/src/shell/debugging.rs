//! Script debugging in the editor: running the active script with its
//! breakpoints, following it while it is paused, and applying what it did.
//!
//! Two kinds of run share the Watch and Call Stack docks, the gutter and the
//! debugger keys ([`Backend`]):
//!
//! - **Edit** (Studio's Edit context): the active script runs against the
//!   place as it stands, the way the Command Bar runs a chunk — on a clone,
//!   on its own thread (see `crate::debugger::session`). The clone replaces
//!   the place once the run is over, as one undo step, unless the place was
//!   edited in the meantime; then the run's changes are dropped and the
//!   Output dock says so, since there is no merging two diverged trees.
//! - **Play**: the scripts of a running test, server and client, stopped by
//!   the hooks they were published with (`shell::play::debug`). It lasts
//!   as long as the test.

mod breakpoint_menu;
mod gutter;
mod panels;

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use rbx_dom::Ref;
use rbx_lua::Resume;

use crate::command_bar::Feedback;
use crate::debugger::session::{Command, Evaluation, Event, Finished, Inspection, Pause, Session};

pub(super) use crate::debugger::session::Pause as DebugPause;
use crate::debugger::Breakpoints;
use crate::script_editor::source;

use super::layout::Panel;
use super::Shell;

/// How often a live run's channel is drained. Quick enough that a step
/// feels immediate; the loop only exists while a run does.
const POLL_INTERVAL: Duration = Duration::from_millis(30);

/// The Shell's debugging state. One field on `Shell`.
#[derive(Default)]
pub(super) struct Debugging {
    pub(super) breakpoints: Breakpoints,
    /// Each open script's text as its breakpoints were last placed against,
    /// so the next edit can move them (see `Breakpoints::follow_edit`).
    texts: HashMap<Ref, String>,
    run: Option<Run>,
    /// My Watches, kept across runs as Studio keeps them.
    watches: Vec<Watch>,
    /// The Watch dock's "add expression" field, made on first use.
    watch_input: Option<Entity<InputState>>,
    watch_subscription: Option<Subscription>,
    clear_watch_input: bool,
    tab: panels::WatchTab,
    menu: Option<breakpoint_menu::Menu>,
    editing: Option<breakpoint_menu::Editing>,
}

/// Where a run's script executes.
enum Backend {
    Edit(Session),
    Play(PlayRun),
}

/// A Play run's pauses: the one the docks show, and others that arrived
/// meanwhile (other threads), shown in turn as each is resumed.
#[derive(Default)]
struct PlayRun {
    current: Option<PlayPause>,
    waiting: std::collections::VecDeque<(PlayPause, Pause)>,
}

/// One paused thread of the game, as the probe numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlayPause {
    pub(super) side: rbx_play::protocol::Side,
    pub(super) id: u64,
    pub(super) script: Ref,
}

/// A run in progress.
struct Run {
    backend: Backend,
    script: Ref,
    /// What the Output dock labels the run's lines with.
    name: String,
    /// `History::revision` when the run started; anything else when it ends
    /// means the place was edited under it.
    revision: u64,
    pause: Option<Pause>,
    /// The Call Stack row the Watch dock is reading; 0, where the script
    /// stopped, at every pause.
    frame: usize,
    /// Whether any pause has been reported yet — the first one opens the
    /// Watch dock.
    paused_once: bool,
}

struct Watch {
    expression: String,
    /// Blank until a pause has evaluated it.
    value: Option<Evaluation>,
}

impl Shell {
    /// Whether the run is a Play session's (see [`Backend`]).
    pub(super) fn debug_in_play(&self) -> bool {
        matches!(
            self.debug.run.as_ref().map(|run| &run.backend),
            Some(Backend::Play(_))
        )
    }

    /// The run, breakpoints and watches as plugins read them.
    pub(super) fn debugger_view(&self) -> rbx_lua::plugin::DebuggerView {
        let mut breakpoints = Vec::new();
        for script in self.debug.breakpoints.scripts() {
            for stored in self.debug.breakpoints.of(script) {
                breakpoints.push((
                    script,
                    stored.breakpoint.line,
                    stored.breakpoint.condition.clone(),
                ));
            }
        }
        rbx_lua::plugin::DebuggerView {
            breakpoints,
            watches: self
                .debug
                .watches
                .iter()
                .map(|watch| {
                    let value = watch
                        .value
                        .as_ref()
                        .map(|value| value.clone().unwrap_or_else(|message| message));
                    (watch.expression.clone(), value)
                })
                .collect(),
            paused: self.debug.run.as_ref().and_then(|run| {
                let pause = run.pause.as_ref()?;
                let stack = pause
                    .stack
                    .iter()
                    .map(|frame| (frame.function.clone(), frame.line))
                    .collect();
                Some((run.script, pause.line, stack))
            }),
            running: self.debug.run.is_some(),
        }
    }

    pub(super) fn debug_add_watch(&mut self, expression: String) {
        if self
            .debug
            .watches
            .iter()
            .any(|w| w.expression == expression)
        {
            return;
        }
        self.debug.watches.push(Watch {
            expression,
            value: None,
        });
        self.request_watch_values();
    }

    pub(super) fn debug_remove_watch(&mut self, expression: &str) {
        self.debug.watches.retain(|w| w.expression != expression);
    }

    pub(super) fn debug_running(&self) -> bool {
        self.debug.run.is_some()
    }

    pub(super) fn debug_paused(&self) -> bool {
        self.debug
            .run
            .as_ref()
            .is_some_and(|run| run.pause.is_some())
    }

    /// Runs the active script tab with its breakpoints.
    pub(super) fn start_debugging(&mut self, cx: &mut Context<Self>) {
        if self.debug.run.is_some() {
            return;
        }
        let Some(script) = self.scripts.tabs.active() else {
            return;
        };
        // The debugger runs against the edited place, which a test stashes.
        if self.live_shown() {
            self.output.push(
                "Debugger",
                Feedback::Warning("Stop the test to debug a script.".to_owned()),
            );
            cx.notify();
            return;
        }
        self.flush_script_edits(cx);
        let Some(text) = source::read(&self.dom, script) else {
            return;
        };
        let name = source::label(&self.dom, script).unwrap_or_default();
        // Whatever the change log holds was reflected already (see
        // `Shell::push_history`); the clone must start with it empty, or the
        // run's own log would carry it a second time.
        self.dom.take_changes();
        let session = Session::start(
            self.dom.clone(),
            self.database.clone(),
            text,
            name.clone(),
            self.debug.breakpoints.for_run(script),
        );
        self.debug.run = Some(Run {
            backend: Backend::Edit(session),
            script,
            name,
            revision: self.history.revision(),
            pause: None,
            frame: 0,
            paused_once: false,
        });
        for watch in &mut self.debug.watches {
            watch.value = None;
        }
        self.spawn_debug_poll(cx);
        self.sync_gutters(cx);
        self.plugins_debugger_changed(cx);
        cx.notify();
    }

    /// Continue or one of the steps; ignored unless paused.
    pub(super) fn resume_debugging(&mut self, resume: Resume, cx: &mut Context<Self>) {
        let Some(run) = self.debug.run.as_mut() else {
            return;
        };
        if run.pause.is_none() {
            return;
        }
        match &mut run.backend {
            Backend::Edit(session) => {
                run.pause = None;
                session.send(Command::Resume(resume));
            }
            Backend::Play(play) => {
                if resume == Resume::Stop {
                    self.play_stop(super::play::session::StopReason::Stopped, cx);
                    return;
                }
                let current = play.current.take();
                let next = play.waiting.pop_front();
                run.pause = None;
                if let Some(paused) = current {
                    self.play_debug_resume(paused, resume);
                }
                if let Some((paused, pause)) = next {
                    self.show_play_pause(paused, pause, cx);
                }
            }
        }
        self.sync_gutters(cx);
        self.plugins_debugger_changed(cx);
        cx.notify();
    }

    /// Stops the run, paused or not. An Edit run still reports back as
    /// finished, so the place is updated through the usual path; a Play
    /// run stops the test, as Studio's Stop does.
    pub(super) fn stop_debugging(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.debug.run.as_mut() else {
            return;
        };
        match &run.backend {
            Backend::Edit(session) => {
                session.stop();
                run.pause = None;
                cx.notify();
            }
            Backend::Play(_) => self.play_stop(super::play::session::StopReason::Stopped, cx),
        }
    }

    /// A test started with scripts to debug: the Play run begins, waiting
    /// for its first pause.
    pub(super) fn begin_play_debugging(&mut self, cx: &mut Context<Self>) {
        // An Edit run in progress keeps going; its stop ends it as usual.
        if self.debug.run.is_some() {
            return;
        }
        self.debug.run = Some(Run {
            backend: Backend::Play(PlayRun::default()),
            // Set by each pause; nothing reads it before the first (the
            // gutter only marks a script while there is a pause).
            script: Ref::new(u32::MAX),
            name: super::play::session::PLAY_SOURCE.to_owned(),
            revision: self.history.revision(),
            pause: None,
            frame: 0,
            paused_once: false,
        });
        for watch in &mut self.debug.watches {
            watch.value = None;
        }
        self.sync_gutters(cx);
        cx.notify();
    }

    /// The test is over: so is its run. Temporary breakpoints of every
    /// debugged script go, as Studio removes them after the session.
    pub(super) fn end_play_debugging(&mut self, scripts: &[Ref], cx: &mut Context<Self>) {
        if matches!(
            self.debug.run.as_ref().map(|run| &run.backend),
            Some(Backend::Play(_))
        ) {
            self.debug.run = None;
        }
        for script in scripts {
            self.debug.breakpoints.end_run(*script);
        }
        // Values read from a game that is gone.
        for watch in &mut self.debug.watches {
            watch.value = None;
        }
        self.sync_gutters(cx);
        self.plugins_debugger_changed(cx);
        cx.notify();
    }

    /// A paused thread of the test: shown at once, or queued behind the
    /// pause the docks already show.
    pub(super) fn on_play_pause(
        &mut self,
        paused: PlayPause,
        pause: Pause,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.debug.run.as_mut() else {
            return;
        };
        let Backend::Play(play) = &mut run.backend else {
            return;
        };
        if play.current.is_some() {
            play.waiting.push_back((paused, pause));
            return;
        }
        self.show_play_pause(paused, pause, cx);
    }

    fn show_play_pause(&mut self, paused: PlayPause, pause: Pause, cx: &mut Context<Self>) {
        let Some(run) = self.debug.run.as_mut() else {
            return;
        };
        let Backend::Play(play) = &mut run.backend else {
            return;
        };
        play.current = Some(paused);
        let first = !std::mem::replace(&mut run.paused_once, true);
        let line = pause.line;
        run.script = paused.script;
        run.pause = Some(pause);
        run.frame = 0;
        self.request_watch_values();
        self.play_reveal_line(paused.script, line, cx);
        if first && !self.is_panel_showing(Panel::Watch) && !self.is_panel_showing(Panel::CallStack)
        {
            self.set_panel_open(Panel::Watch, true, cx);
        }
        self.sync_gutters(cx);
        self.plugins_debugger_changed(cx);
        cx.notify();
    }

    /// The probe's answer to the watch expressions of the shown pause.
    pub(super) fn on_play_values(
        &mut self,
        side: rbx_play::protocol::Side,
        id: u64,
        values: Vec<(String, Evaluation)>,
    ) {
        let Some(run) = self.debug.run.as_ref() else {
            return;
        };
        let Backend::Play(play) = &run.backend else {
            return;
        };
        if play.current.is_none_or(|p| p.side != side || p.id != id) || run.frame != 0 {
            return;
        }
        for (expression, value) in values {
            for watch in &mut self.debug.watches {
                if watch.expression == expression {
                    watch.value = Some(value.clone());
                }
            }
        }
    }

    fn spawn_debug_poll(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            match shell.update(cx, |shell, cx| shell.drain_debug_events(cx)) {
                Ok(true) => {}
                _ => break,
            }
        })
        .detach();
    }

    /// Handles whatever the run reported; `false` once there is no run left
    /// to poll.
    fn drain_debug_events(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(Backend::Edit(session)) = self.debug.run.as_ref().map(|run| &run.backend) else {
            return false;
        };
        let events = session.events();

        if events.is_empty() {
            return true;
        }
        for event in events {
            match event {
                Event::Paused(pause) => self.on_debug_pause(pause, cx),
                Event::Inspected(inspection) => self.on_inspected(inspection),
                Event::Finished(finished) => {
                    self.finish_debugging(finished, cx);
                    cx.notify();
                    return false;
                }
            }
        }
        cx.notify();
        true
    }

    fn on_debug_pause(&mut self, pause: Pause, cx: &mut Context<Self>) {
        let Some(run) = self.debug.run.as_mut() else {
            return;
        };
        let script = run.script;
        let first = !std::mem::replace(&mut run.paused_once, true);
        if !pause.output.is_empty() {
            self.output
                .push(&run.name, Feedback::Output(pause.output.join("\n")));
        }
        let line = pause.line;
        run.pause = Some(pause);
        run.frame = 0;
        self.request_watch_values();

        // Where the script stopped: its tab in front, the caret on the line
        // (which also scrolls it into view), the Watch dock up.
        self.reveal_line(script, line, cx);
        if first && !self.is_panel_showing(Panel::Watch) && !self.is_panel_showing(Panel::CallStack)
        {
            self.set_panel_open(Panel::Watch, true, cx);
        }
        self.sync_gutters(cx);
        self.plugins_debugger_changed(cx);
    }

    /// Clicking a Call Stack row: the Watch dock reads that frame, and the
    /// caret goes to the line it is on.
    pub(super) fn select_frame(&mut self, frame: usize, cx: &mut Context<Self>) {
        let Some(run) = self.debug.run.as_mut() else {
            return;
        };
        let Some(line) = run
            .pause
            .as_ref()
            .and_then(|pause| pause.stack.get(frame))
            .map(|entry| entry.line)
        else {
            return;
        };
        run.frame = frame;
        let frame_script = run
            .pause
            .as_ref()
            .and_then(|pause| pause.scripts.get(frame).copied());
        let play = matches!(run.backend, Backend::Play(_));
        let script = run.script;
        self.request_watch_values();
        match frame_script {
            Some(Some(script)) => self.play_reveal_line(script, line, cx),
            // A frame in a script the editor cannot place (a Roblox core
            // script, or one published without hooks).
            Some(None) => {}
            None if play => self.play_reveal_line(script, line, cx),
            None => self.reveal_line(script, line, cx),
        }
        cx.notify();
    }

    fn reveal_line(&mut self, script: Ref, line: u32, cx: &mut Context<Self>) {
        self.activate_script(script, cx);
        if let Some(open) = self.scripts.open.get(&script) {
            open.state.update(cx, |state, cx| {
                let text = state.value().to_string();
                let start = line_start(&text, line);
                state.set_selected_range(start..start, cx);
            });
        }
    }

    /// A frame's variables and watch values. One that answers a frame no
    /// longer selected — the answer crossed another click — is dropped.
    fn on_inspected(&mut self, inspection: Inspection) {
        let Some(run) = self.debug.run.as_mut() else {
            return;
        };
        if inspection.frame != run.frame {
            return;
        }
        if let Some(pause) = run.pause.as_mut() {
            pause.variables = inspection.variables;
        }
        for (expression, value) in inspection.values {
            for watch in &mut self.debug.watches {
                if watch.expression == expression {
                    watch.value = Some(value.clone());
                }
            }
        }
    }

    /// Asks the paused run for the selected frame's variables and every
    /// watch's value in it.
    fn request_watch_values(&mut self) {
        let Some(run) = self.debug.run.as_ref().filter(|run| run.pause.is_some()) else {
            return;
        };
        let expressions: Vec<String> = self
            .debug
            .watches
            .iter()
            .map(|watch| watch.expression.clone())
            .collect();
        match &run.backend {
            Backend::Edit(session) => session.send(Command::Inspect {
                frame: run.frame,
                expressions,
            }),
            Backend::Play(play) => {
                let Some(paused) = play.current else {
                    return;
                };
                if run.frame == 0 {
                    self.play_debug_watch(paused, expressions);
                } else {
                    // The probe only sees the paused frame's locals.
                    for watch in &mut self.debug.watches {
                        watch.value = Some(Err(
                            "Only the frame where the game stopped can be read during Play"
                                .to_owned(),
                        ));
                    }
                }
            }
        }
    }

    fn finish_debugging(&mut self, finished: Finished, cx: &mut Context<Self>) {
        let Some(run) = self.debug.run.take() else {
            return;
        };
        self.debug.breakpoints.end_run(run.script);
        if !finished.output.is_empty() {
            self.output
                .push(&run.name, Feedback::Output(finished.output.join("\n")));
        }
        match finished.result {
            // Asked for, so not an error — and its traceback would only
            // point at wherever the script happened to be.
            Err(message) if message.contains(rbx_lua::STOPPED) => self
                .output
                .push(&run.name, Feedback::Output(rbx_lua::STOPPED.to_owned())),
            Err(message) => self.output.push(&run.name, Feedback::Error(message)),
            Ok(()) => {}
        }
        if let Some(dom) = finished.dom {
            if self.history.revision() == run.revision && !self.live_shown() {
                let before = std::mem::replace(&mut self.dom, dom);
                self.push_history_snapshot(before);
                self.rebuild_after_script(cx);
            } else {
                self.output.push_warning(&format!(
                    "The place was edited while {} was being debugged, so the \
                     script's own changes were discarded",
                    run.name
                ));
            }
        }
        self.sync_gutters(cx);
        self.plugins_debugger_changed(cx);
    }

    /// Moves `script`'s breakpoints with whatever its editor's text did
    /// since last time. Called on every change to an open tab, typed or
    /// re-seeded from the DOM; the first call only records the text.
    pub(super) fn follow_script_edit(&mut self, script: Ref, cx: &mut Context<Self>) {
        let Some(open) = self.scripts.open.get(&script) else {
            return;
        };
        let text = open.state.read(cx).value().to_string();
        let Some(old) = self.debug.texts.insert(script, text.clone()) else {
            return;
        };
        if old != text && self.debug.breakpoints.of(script).next().is_some() {
            self.debug.breakpoints.follow_edit(script, &old, &text);
            self.sync_gutters(cx);
            self.plugins_debugger_changed(cx);
        }
    }

    /// F5 / F9 / F10 / F11 and their Shift variants — Studio's debugger keys
    /// — while the Script Editor has focus. `true` if the key was one.
    pub(super) fn handle_debug_key(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        let shift = keystroke.modifiers.shift;
        let paused = self.debug_paused();
        // F10 and F11 are only claimed while paused: F10 is also the
        // window's own key for the menu bar (see `shell::save`).

        match (keystroke.key.as_str(), shift) {
            ("f5", false) if paused => self.resume_debugging(Resume::Continue, cx),
            ("f5", false) if !self.debug_running() => self.start_debugging(cx),
            ("f5", true) if self.debug_running() => self.stop_debugging(cx),
            ("f10", false) if paused => self.resume_debugging(Resume::StepOver, cx),
            ("f11", false) if paused => self.resume_debugging(Resume::StepInto, cx),
            ("f11", true) if paused => self.resume_debugging(Resume::StepOut, cx),
            ("f9", false) => self.toggle_breakpoint_at_cursor(cx),
            _ => return false,
        }
        true
    }
}

/// The byte offset where 1-based `line` starts, clamped to the text.
fn line_start(text: &str, line: u32) -> usize {
    if line <= 1 {
        return 0;
    }
    text.match_indices('\n')
        .nth(line as usize - 2)
        .map_or(text.len(), |(index, _)| index + 1)
}

#[cfg(test)]
mod tests {
    use super::line_start;

    #[test]
    fn line_start_finds_each_line_and_clamps_past_the_end() {
        let text = "a\nbc\n\nd";
        assert_eq!(line_start(text, 1), 0);
        assert_eq!(line_start(text, 2), 2);
        assert_eq!(line_start(text, 3), 5);
        assert_eq!(line_start(text, 4), 6);
        assert_eq!(line_start(text, 9), text.len());
    }
}
