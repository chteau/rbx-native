//! Script debugging in the editor: running the active script with its
//! breakpoints, following it while it is paused, and applying what it did.
//!
//! There is no Play mode to debug inside, so a debug run is what Studio
//! calls the Edit context: the script runs against the place as it stands,
//! the way the Command Bar runs a chunk — on a clone, on its own thread (see
//! `crate::debugger::session`). The clone replaces the place once the run
//! is over, as one undo step, unless the place was edited in the meantime;
//! then the run's changes are dropped and the Output dock says so, since
//! there is no merging two diverged trees.

mod breakpoint_menu;
mod gutter;
mod panels;

use std::time::Duration;

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use rbx_dom::Ref;
use rbx_lua::Resume;

use crate::command_bar::Feedback;
use crate::debugger::session::{Command, Evaluation, Event, Finished, Pause, Session};
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

/// A run in progress.
struct Run {
    session: Session,
    script: Ref,
    /// What the Output dock labels the run's lines with.
    name: String,
    /// `History::revision` when the run started; anything else when it ends
    /// means the place was edited under it.
    revision: u64,
    pause: Option<Pause>,
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
            session,
            script,
            name,
            revision: self.history.revision(),
            pause: None,
            paused_once: false,
        });
        for watch in &mut self.debug.watches {
            watch.value = None;
        }
        self.spawn_debug_poll(cx);
        self.sync_gutters(cx);
        cx.notify();
    }

    /// Continue or one of the steps; ignored unless paused.
    pub(super) fn resume_debugging(&mut self, resume: Resume, cx: &mut Context<Self>) {
        let Some(run) = self.debug.run.as_mut() else {
            return;
        };
        if run.pause.take().is_some() {
            run.session.send(Command::Resume(resume));
            self.sync_gutters(cx);
            cx.notify();
        }
    }

    /// Stops the run, paused or not. The run still reports back as
    /// finished, so the place is updated through the usual path.
    pub(super) fn stop_debugging(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = self.debug.run.as_mut() {
            run.session.stop();
            run.pause = None;
            cx.notify();
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
        let Some(run) = self.debug.run.as_ref() else {
            return false;
        };
        let events = run.session.events();
        if events.is_empty() {
            return true;
        }
        for event in events {
            match event {
                Event::Paused(pause) => self.on_debug_pause(pause, cx),
                Event::Evaluated(values) => self.on_watches_evaluated(values),
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
        self.request_watch_values();

        // Where the script stopped: its tab in front, the caret on the line
        // (which also scrolls it into view), the Watch dock up.
        self.activate_script(script, cx);
        if let Some(open) = self.scripts.open.get(&script) {
            open.state.update(cx, |state, cx| {
                let text = state.value().to_string();
                let start = line_start(&text, line);
                state.set_selected_range(start..start, cx);
            });
        }
        if first && !self.is_panel_showing(Panel::Watch) && !self.is_panel_showing(Panel::CallStack)
        {
            self.set_panel_open(Panel::Watch, true, cx);
        }
        self.sync_gutters(cx);
    }

    fn on_watches_evaluated(&mut self, values: Vec<(String, Evaluation)>) {
        for (expression, value) in values {
            for watch in &mut self.debug.watches {
                if watch.expression == expression {
                    watch.value = Some(value.clone());
                }
            }
        }
    }

    /// Asks the paused run for every watch's value.
    fn request_watch_values(&self) {
        let Some(run) = self.debug.run.as_ref().filter(|run| run.pause.is_some()) else {
            return;
        };
        if !self.debug.watches.is_empty() {
            let expressions = self
                .debug
                .watches
                .iter()
                .map(|watch| watch.expression.clone())
                .collect();
            run.session.send(Command::Evaluate(expressions));
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
        if let Err(message) = finished.result {
            self.output.push(&run.name, Feedback::Error(message));
        }
        if let Some(dom) = finished.dom {
            if self.history.revision() == run.revision {
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
    }

    /// F5 / F9 / F10 / F11 and their Shift variants — Studio's debugger keys
    /// — while the Script Editor has focus. `true` if the key was one.
    pub(super) fn handle_debug_key(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        let shift = keystroke.modifiers.shift;
        match (keystroke.key.as_str(), shift) {
            ("f5", false) if self.debug_paused() => self.resume_debugging(Resume::Continue, cx),
            ("f5", false) if !self.debug_running() => self.start_debugging(cx),
            ("f5", true) => self.stop_debugging(cx),
            ("f10", false) => self.resume_debugging(Resume::StepOver, cx),
            ("f11", false) => self.resume_debugging(Resume::StepInto, cx),
            ("f11", true) => self.resume_debugging(Resume::StepOut, cx),
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
