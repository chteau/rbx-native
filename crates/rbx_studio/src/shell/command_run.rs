//! Running Command Bar scripts the way Studio's bar does: one persistent
//! `rbx_lua::Runtime` over the place, a run that yields (`task.wait`, signals)
//! is resumed from the UI loop between frames, and what each resume printed
//! and changed reaches the Output panel, the Explorer and the viewport as it
//! happens — the editor stays live while a script waits.

use std::time::Duration;

use gpui_kit::{Context, Task, Window};
use rbx_lua::{RunState, Runtime};

use crate::command_bar::{stream, Feedback, RunUndo};

use super::Shell;

/// The shortest sleep between resumes: a `task.wait()` of zero would
/// otherwise spin the UI thread.
const FRAME: Duration = Duration::from_millis(16);

/// Everything the shell keeps about Command Bar runs.
#[derive(Default)]
pub(super) struct CommandRun {
    /// Built on the first run, lent the place's DOM for each resume only.
    runtime: Option<Runtime>,
    /// The timer loop resuming a waiting run; dropping it stops the loop.
    timer: Option<Task<()>>,
    /// The undo step the newest run's changes belong to.
    undo: Option<RunUndo>,
    /// The newest run's source, the heading its Output rows carry. Threads of
    /// an earlier run that are still waiting report under it too.
    source: String,
    /// Whether the newest run has put a row in the Output panel yet.
    emitted: bool,
}

impl Shell {
    /// Enter in the Command Bar: runs the input's text, then clears it unless
    /// it did not compile, so the user can fix it.
    pub(super) fn run_typed_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.command_bar.input().read(cx).value().to_string();
        if self.run_command(&source, cx) {
            self.command_bar
                .input()
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    /// Up/Down in the Command Bar: swaps the input's text for the older or
    /// newer submitted command.
    pub(super) fn walk_command_history(
        &mut self,
        older: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.command_bar.input().read(cx).value().to_string();
        let text = if older {
            self.command_bar.recall_older(&current)
        } else {
            self.command_bar.recall_newer()
        };
        if let Some(text) = text {
            self.command_bar
                .input()
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
    }

    /// Starts `source` as a run of its own (any run still waiting keeps going)
    /// and resumes once at once; a run that yields is then resumed from a
    /// timer. Returns whether `source` compiled.
    pub(super) fn run_command(&mut self, source: &str, cx: &mut Context<Self>) -> bool {
        let compiled = rbx_lua::compile_check(source, "=Command bar").is_ok();
        self.command_bar.remember(source);
        // Snapshotted before the run touches anything, whether or not it ends
        // up mutating — see `shell::history`. The run's changes all join this
        // one entry (see `command_bar::undo`).
        self.push_history();
        self.command_run.undo = Some(RunUndo::new(self.history.revision()));
        self.command_run.source = source.to_owned();
        self.command_run.emitted = false;

        if self.command_run.runtime.is_none() {
            match Runtime::lent(&mut self.dom, self.database.clone()) {
                Ok(runtime) => self.command_run.runtime = Some(runtime),
                Err(err) => {
                    self.show_run_error(&err.to_string());
                    cx.notify();
                    return compiled;
                }
            }
        }
        if let Some(runtime) = self.command_run.runtime.as_mut() {
            runtime.start(source);
        }
        if let Some(wait) = self.command_step(cx) {
            self.arm_command_timer(wait, cx);
        }
        compiled
    }

    /// Stops whatever a run left waiting: its sleeping threads, timers and
    /// event connections. Called when the place closes.
    pub(super) fn cancel_command_run(&mut self) {
        if let Some(runtime) = self.command_run.runtime.as_mut() {
            runtime.cancel();
        }
        self.command_run.timer = None;
        self.command_run.undo = None;
    }

    /// Sleeps `wait` and resumes, again and again until nothing is left
    /// waiting. A newer run replaces the loop, so its shorter sleep is not
    /// stuck behind an older one.
    fn arm_command_timer(&mut self, mut wait: Duration, cx: &mut Context<Self>) {
        self.command_run.timer = Some(cx.spawn(async move |shell, cx| {
            loop {
                cx.background_executor().timer(wait).await;
                match shell.update(cx, |shell, cx| shell.command_step(cx)) {
                    Ok(Some(next)) => wait = next,
                    _ => break,
                }
            }
            let _ = shell.update(cx, |shell, _| shell.command_run.timer = None);
        }));
    }

    /// One resume: runs the threads that are due, shows what they printed,
    /// and reflects what they changed. The sleep until the next resume, or
    /// `None` once the run is over.
    fn command_step(&mut self, cx: &mut Context<Self>) -> Option<Duration> {
        // Something else pushed an undo entry since the last step (an edit, an
        // undo, a second command): this step's changes open an entry of their
        // own instead of landing in, or overwriting, that one.
        let revision = self.history.revision();
        if self
            .command_run
            .undo
            .as_ref()
            .is_some_and(|undo| undo.interrupted(revision))
        {
            self.push_history();
            self.command_run.undo = Some(RunUndo::new(self.history.revision()));
        }

        let runtime = self.command_run.runtime.as_mut()?;
        let step = runtime.resume_in(&mut self.dom);

        let source = self.command_run.source.clone();
        for (kind, range) in stream::segments(step.output.kinds()) {
            let feedback = stream::feedback_for(kind, &step.output.lines()[range.clone()]);
            match kind {
                rbx_lua::LineKind::Print => self.output.push_run(
                    &source,
                    feedback.clone(),
                    step.output.printed()[range].to_vec(),
                ),
                _ => self.output.push(&source, feedback.clone()),
            }
            self.command_bar.set_feedback(feedback);
            self.command_run.emitted = true;
        }

        let wait = match step.state {
            RunState::Pending { wake_in } => Some(wake_in.max(FRAME)),
            RunState::Finished => {
                if !self.command_run.emitted {
                    // A silent success still says it ran.
                    let feedback = Feedback::Output(String::new());
                    self.output.push_run(&source, feedback.clone(), Vec::new());
                    self.command_bar.set_feedback(feedback);
                }
                None
            }
            RunState::Failed(err) => {
                self.show_run_error(&err.to_string());
                None
            }
        };
        self.reflect_command_changes(cx);
        if wait.is_none() {
            self.command_run.undo = None;
        }
        cx.notify();
        wait
    }

    fn show_run_error(&mut self, message: &str) {
        let feedback = Feedback::Error(message.to_owned());
        self.output.push(&self.command_run.source, feedback.clone());
        self.command_run.emitted = true;
        self.command_bar.set_feedback(feedback);
    }

    /// Shows this step's DOM changes in the Explorer and the viewport — also
    /// after an error, since what the script did before it failed is kept —
    /// and files them under the run's undo step.
    fn reflect_command_changes(&mut self, cx: &mut Context<Self>) {
        let changes = self.dom.take_changes();
        if changes.is_empty() {
            return;
        }
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        if let Some(undo) = self.command_run.undo.as_mut() {
            let log = undo.absorb(changes).to_vec();
            self.record_history_change(log);
        }
    }
}
