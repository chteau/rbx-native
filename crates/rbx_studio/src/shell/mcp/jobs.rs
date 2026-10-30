//! The `crate::mcp::Job`s a tool call hands the shell: each waits here,
//! polled on every drain, until its work is done or its deadline passes.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::{AppContext as _, Context};
use rbx_play::protocol::{Side, Uplink};
use serde_json::Value;

use crate::command_bar::Feedback;
use crate::mcp::endpoint::Request;
use crate::mcp::protocol::job_reply;
use crate::mcp::{text_content, DataModel, Job, JobOutcome};

use super::super::output::OutputLog;
use super::super::play::session::{StopReason, PLAY_SOURCE};
use super::super::play::Phase;
use super::super::Shell;
use super::capture;

const LUAU_DEADLINE: Duration = Duration::from_secs(100);
const PLAY_DEADLINE: Duration = Duration::from_secs(200);
pub(super) const CAPTURE_DEADLINE: Duration = Duration::from_secs(20);

/// get_console_output keeps the newest rows whose lengths add up to at
/// most this many characters, as Studio's does.
pub(super) const CONSOLE_CAP: usize = 20_000;
pub(super) const TRUNCATED: &str = "[TRUNCATED DUE TO LENGTH LIMIT]";

/// What a waiting call is waiting on.
pub(super) enum Wait {
    /// An Edit run in the Command Bar's runtime.
    Luau(rbx_lua::Reported),
    /// The ack of an `Exec` sent to the game.
    Ack(u64),
    /// play_start, until it reaches Running or falls back to Idle; `begun`
    /// once play_start has been called, so the Idle before it is not taken
    /// for a failed start.
    PlayStart { begun: bool },
    /// screen_capture's steps.
    Capture(capture::Step),
}

struct Pending {
    request: Request,
    id: Value,
    deadline: Instant,
    timeout: &'static str,
    wait: Wait,
}

/// Everything the shell keeps about waiting MCP calls.
#[derive(Default)]
pub(in crate::shell) struct Jobs {
    pending: Vec<Pending>,
    /// Acks of waited-on commands, kept until the next poll takes them.
    pub(super) acks: HashMap<u64, Result<String, String>>,
}

impl Jobs {
    pub(super) fn waits_on(&self, id: u64) -> bool {
        self.pending.iter().any(|pending| match &pending.wait {
            Wait::Ack(waited) => *waited == id,
            Wait::Capture(step) => step.ack() == Some(id),
            _ => false,
        })
    }
}

/// What start_stop_play does from `phase`.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PlayAction {
    Answer(Result<String, String>),
    Start,
    Stop,
}

pub(super) fn play_action(start: bool, phase: &Phase) -> PlayAction {
    match (start, phase) {
        (true, Phase::Running) => PlayAction::Answer(Ok("Game Started".to_owned())),
        (true, Phase::Idle) => PlayAction::Start,
        (_, Phase::Starting(_)) => {
            PlayAction::Answer(Ok("Start play hasn't finished yet".to_owned()))
        }
        (false, Phase::Idle) => PlayAction::Answer(Ok("Game Stopped".to_owned())),
        (false, Phase::Running) => PlayAction::Stop,
    }
}

fn feedback_text(feedback: &Feedback) -> Option<&str> {
    match feedback {
        Feedback::Idle => None,
        Feedback::Output(text)
        | Feedback::Error(text)
        | Feedback::Warning(text)
        | Feedback::Info(text) => Some(text),
    }
}

/// get_console_output's text, one printed line per line. Like Studio's, it
/// keeps whole lines, newest first, while their lengths (not the line
/// breaks) add up to at most `CONSOLE_CAP`, and puts the truncation marker
/// on a line of its own in front when that left any out: a single line
/// longer than the cap leaves only the marker. A run's prints share one
/// Output row here, so rows are split into the lines Studio keeps apart.
pub(super) fn console_text(log: &OutputLog) -> String {
    let rows: Vec<&str> = log
        .entries()
        .iter()
        .filter_map(|entry| feedback_text(entry.feedback()))
        // A silent Command Bar run leaves an empty row; Studio has none.
        .filter(|text| !text.is_empty())
        .flat_map(str::lines)
        .collect();
    let mut budget = CONSOLE_CAP;
    let kept = rows
        .iter()
        .rev()
        .take_while(|row| {
            let length = row.chars().count();
            let fits = length <= budget;
            budget = budget.saturating_sub(length);
            fits
        })
        .count();
    let tail = &rows[rows.len() - kept..];
    if kept == rows.len() {
        return tail.join("\n");
    }
    std::iter::once(TRUNCATED)
        .chain(tail.iter().copied())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The newest Play error in Output, why a start fell back to Idle.
fn last_play_error(log: &OutputLog) -> Option<String> {
    log.entries()
        .iter()
        .rev()
        .filter(|entry| entry.source() == PLAY_SOURCE)
        .find_map(|entry| match entry.feedback() {
            Feedback::Error(text) => Some(text.clone()),
            _ => None,
        })
}

fn text_outcome(result: Result<String, String>) -> JobOutcome {
    result.map(|text| vec![text_content(&text)])
}

impl Shell {
    /// Starts `job` for `request`, answering at once when it can.
    pub(super) fn mcp_job(
        &mut self,
        request: Request,
        id: Value,
        job: Job,
        cx: &mut Context<Self>,
    ) {
        let (wait, deadline, timeout) = match job {
            Job::Console => {
                let text = console_text(&self.output);
                return request.answer(Some(job_reply(id, Ok(vec![text_content(&text)]))));
            }
            Job::Save { path } => {
                let saved = self.save_for_mcp(path.as_deref(), cx);
                return request.answer(Some(job_reply(id, text_outcome(saved))));
            }
            Job::Luau { code, datamodel } => {
                let wait = match datamodel {
                    DataModel::Edit => Ok(Wait::Luau(self.run_command_reported(&code, cx))),
                    DataModel::Client => self.mcp_exec(Side::Client, &code),
                    DataModel::Server => self.mcp_exec(Side::Server, &code),
                };
                match wait {
                    Ok(wait) => (wait, LUAU_DEADLINE, "the code is still running after 100 s"),
                    Err(err) => return request.answer(Some(job_reply(id, Err(err)))),
                }
            }
            Job::Play { start } => match play_action(start, &self.play.phase) {
                PlayAction::Answer(result) => {
                    return request.answer(Some(job_reply(id, text_outcome(result))));
                }
                PlayAction::Stop => {
                    self.play_stop(StopReason::Stopped, cx);
                    let reply = job_reply(id, text_outcome(Ok("Game Stopped".to_owned())));
                    return request.answer(Some(reply));
                }
                PlayAction::Start => {
                    self.mcp_play_start(cx);
                    (
                        Wait::PlayStart { begun: false },
                        PLAY_DEADLINE,
                        "Start play hasn't finished yet",
                    )
                }
            },
            Job::Capture { camera } => match self.capture_begin(camera, cx) {
                Ok(step) => (
                    Wait::Capture(step),
                    CAPTURE_DEADLINE,
                    "the screen capture did not finish within 20 s",
                ),
                Err(err) => return request.answer(Some(job_reply(id, Err(err)))),
            },
        };
        self.mcp_jobs.pending.push(Pending {
            request,
            id,
            deadline: Instant::now() + deadline,
            timeout,
            wait,
        });
    }

    /// Sends execute_luau's code to the game; what to wait for.
    pub(super) fn mcp_exec(&mut self, side: Side, code: &str) -> Result<Wait, String> {
        let body = rbx_play::exec::command(side, code)?;
        self.play_send(body)
            .map(Wait::Ack)
            .ok_or_else(|| "the game is not running".to_owned())
    }

    /// play_start needs the window, which a timer tick does not have.
    fn mcp_play_start(&mut self, cx: &mut Context<Self>) {
        let handle = self.window_handle;
        cx.spawn(async move |this, cx| {
            let _ = cx.update_window(handle, |_, window, cx| {
                this.update(cx, |shell, cx| {
                    shell.play_start(window, cx);
                    for pending in &mut shell.mcp_jobs.pending {
                        if let Wait::PlayStart { begun } = &mut pending.wait {
                            *begun = true;
                        }
                    }
                })
            });
        })
        .detach();
    }

    /// Takes the game's ack when an MCP call waits on it.
    pub(in crate::shell) fn mcp_uplink(&mut self, event: &Uplink) -> bool {
        let Uplink::Ack { id, ok, result } = event else {
            return false;
        };
        if !self.mcp_jobs.waits_on(*id) {
            return false;
        }
        let result = result.clone();
        self.mcp_jobs
            .acks
            .insert(*id, if *ok { Ok(result) } else { Err(result) });
        true
    }

    /// Answers every waiting call whose work is done or whose time is up.
    pub(super) fn mcp_poll(&mut self, cx: &mut Context<Self>) {
        let pending = std::mem::take(&mut self.mcp_jobs.pending);
        let now = Instant::now();
        for mut job in pending {
            let outcome = match self.mcp_progress(&mut job.wait, cx) {
                Some(outcome) => Some(outcome),
                None if now >= job.deadline => Some(Err(job.timeout.to_owned())),
                None => None,
            };
            match outcome {
                Some(outcome) => job.request.answer(Some(job_reply(job.id, outcome))),
                None => self.mcp_jobs.pending.push(job),
            }
        }
    }

    fn mcp_progress(&mut self, wait: &mut Wait, cx: &mut Context<Self>) -> Option<JobOutcome> {
        match wait {
            Wait::Luau(reported) => reported.borrow().clone().map(text_outcome),
            Wait::Ack(id) => self.mcp_jobs.acks.remove(id).map(text_outcome),
            Wait::PlayStart { begun } => match self.play.phase {
                Phase::Running => Some(text_outcome(Ok("Game Started".to_owned()))),
                Phase::Idle if *begun => Some(Err(last_play_error(&self.output)
                    .unwrap_or_else(|| "Play did not start".to_owned()))),
                _ => None,
            },
            Wait::Capture(step) => self.capture_progress(step, cx),
        }
    }
}
