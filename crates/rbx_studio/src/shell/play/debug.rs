//! Script debugging inside a Play session: the editor's half.
//!
//! A game script can't be stopped from outside or read its own locals
//! (Roblox's `debug` library has no `getlocal`), so when Play publishes,
//! every script with breakpoints is rewritten with hooks
//! (`rbx_play::instrument`, through `session::instrument_scripts`). A hook
//! reports to the probe's `__rbxnative_debug` module, which stops that
//! thread at an active breakpoint or after a step and waits for the editor.
//! Other threads, physics and rendering keep going: only the stopped
//! script waits, where Studio freezes the whole session.
//!
//! - **Breakpoints** active when Play starts are built into the published
//!   copy, so code that runs at startup stops too. Turning one on or off,
//!   adding or removing one in a debugged script reaches the game at once
//!   (`play_debug_sync`, an `rbx_play::protocol::CommandBody::DebugSet` per
//!   side, filtered by each breakpoint's Trigger At contexts). Conditions
//!   and log messages are compiled into the copy, so editing one, or adding
//!   breakpoints to a script that had none, takes effect at the next Play;
//!   Output says so.
//! - **Pauses** (`Uplink::Paused`) open the script at the line and fill the
//!   Watch and Call Stack docks through `shell::debugging`'s Play backend.
//!   Only the paused frame has variables (the hook captures its locals).
//!   Watch expressions are evaluated in the game (`DebugWatch`): any Luau
//!   on the server, names and fields on the client (no `loadstring` there).

use std::collections::HashSet;

use gpui_kit::*;
use rbx_dom::Ref;
use rbx_lua::{Frame, Resume, Variable};
use rbx_play::instrument::{resolve_line, Hook};
use rbx_play::protocol::{CommandBody, DebugAction, DebugLine, Side, Uplink};

use super::super::debugging::{DebugPause, PlayPause};
use super::super::output::game::Context as OutputContext;
use super::super::Shell;
use super::session::{DebugRequest, DebugScript, PLAY_SOURCE};
use crate::command_bar::Feedback;

#[derive(Default)]
pub(crate) struct PlayDebug {
    /// The scripts the running test was published with hooks for.
    pub(super) scripts: Vec<DebugScript>,
    /// From `prepare`, until the session runs.
    pub(super) pending: Vec<DebugScript>,
    /// The active lines last sent to the server and to the client.
    sent: Option<(Vec<DebugLine>, Vec<DebugLine>)>,
    /// Scripts already told "applies at the next Play" this session.
    noted: HashSet<Ref>,
}

impl PlayDebug {
    fn id_of(&self, script: Ref) -> Option<&DebugScript> {
        self.scripts.iter().find(|s| s.script == script)
    }

    fn script_of(&self, id: u32) -> Option<Ref> {
        self.scripts.iter().find(|s| s.id == id).map(|s| s.script)
    }
}

fn action(resume: Resume) -> DebugAction {
    match resume {
        Resume::StepInto => DebugAction::StepInto,
        Resume::StepOver => DebugAction::StepOver,
        Resume::StepOut => DebugAction::StepOut,
        Resume::Continue | Resume::Stop => DebugAction::Continue,
    }
}

impl Shell {
    /// What `session::prepare` instruments: every script of the edited
    /// place with breakpoints (enabled or not, so turning one on later
    /// works), each breakpoint active on a side when it is enabled and
    /// triggers there.
    pub(in crate::shell) fn play_debug_requests(&self) -> Vec<DebugRequest> {
        let place = self.edit_dom();
        let mut scripts: Vec<Ref> = self
            .debug
            .breakpoints
            .scripts()
            .filter(|script| {
                crate::script_editor::source::is_script(place, &self.database, *script)
            })
            .collect();
        scripts.sort();
        scripts
            .into_iter()
            .map(|script| DebugRequest {
                script,
                hooks: self
                    .debug
                    .breakpoints
                    .of(script)
                    .map(|stored| Hook {
                        line: stored.breakpoint.line,
                        condition: stored.breakpoint.condition.clone(),
                        log_message: stored.breakpoint.log_message.clone(),
                        continue_execution: stored.breakpoint.continue_execution,
                        server: stored.enabled && stored.contexts.server,
                        client: stored.enabled && stored.contexts.client,
                    })
                    .collect(),
            })
            .collect()
    }

    /// The test is running: its debugged scripts become live, and so does
    /// the debugger's Play run when there are any.
    pub(in crate::shell) fn play_debug_begin(&mut self, cx: &mut Context<Self>) {
        let debug = &mut self.play.debug;
        debug.scripts = std::mem::take(&mut debug.pending);
        debug.sent = None;
        debug.noted.clear();
        if !self.play.debug.scripts.is_empty() {
            self.begin_play_debugging(cx);
        }
        self.play_debug_sync();
    }

    pub(in crate::shell) fn play_debug_end(&mut self, cx: &mut Context<Self>) {
        let scripts: Vec<Ref> = self.play.debug.scripts.iter().map(|s| s.script).collect();
        self.play.debug = PlayDebug::default();
        self.end_play_debugging(&scripts, cx);
    }

    /// Sends each side's active breakpoint lines when they changed. Called
    /// with every gutter refresh, which follows every breakpoint change.
    pub(in crate::shell) fn play_debug_sync(&mut self) {
        if self.play.phase != super::Phase::Running {
            return;
        }
        let mut server = Vec::new();
        let mut client = Vec::new();
        let mut late = Vec::new();
        for script in self.debug.breakpoints.scripts().collect::<Vec<_>>() {
            let Some(debugged) = self.play.debug.id_of(script) else {
                if self.debug.breakpoints.of(script).any(|s| s.enabled) {
                    late.push(script);
                }
                continue;
            };
            for stored in self.debug.breakpoints.of(script).filter(|s| s.enabled) {
                let Some(line) = resolve_line(&debugged.lines, stored.breakpoint.line) else {
                    continue;
                };
                let at = DebugLine {
                    script: debugged.id,
                    line,
                };
                if stored.contexts.server {
                    server.push(at);
                }
                if stored.contexts.client {
                    client.push(at);
                }
            }
        }
        for script in late {
            if self.play.debug.noted.insert(script) {
                let name = crate::script_editor::source::label(self.edit_dom(), script)
                    .unwrap_or_default();
                self.output.push(
                    PLAY_SOURCE,
                    Feedback::Output(format!(
                        "Breakpoints in {name} apply from the next Play: scripts are prepared \
                         for debugging when Play starts."
                    )),
                );
            }
        }
        let lines = (server, client);
        if self.play.debug.sent.as_ref() == Some(&lines) {
            return;
        }
        if let Some(link) = self.play.link.as_ref() {
            link.channel.send(CommandBody::DebugSet {
                side: Side::Server,
                lines: lines.0.clone(),
            });
            link.channel.send(CommandBody::DebugSet {
                side: Side::Client,
                lines: lines.1.clone(),
            });
            self.play.debug.sent = Some(lines);
        }
    }

    /// A breakpoint's condition or log message changed during a test: the
    /// published copy still has the old one compiled in.
    pub(in crate::shell) fn play_debug_edited(&mut self, script: Ref) {
        if self.play.phase == super::Phase::Running && self.play.debug.id_of(script).is_some() {
            self.output.push(
                PLAY_SOURCE,
                Feedback::Output(
                    "Breakpoint conditions and log messages apply from the next Play; turning \
                     breakpoints on and off applies now."
                        .to_owned(),
                ),
            );
        }
    }

    /// A debugger message from the probe; false for any other message.
    pub(in crate::shell) fn play_debug_event(
        &mut self,
        event: &Uplink,
        cx: &mut Context<Self>,
    ) -> bool {
        match event {
            Uplink::Paused {
                side,
                pause,
                script,
                line,
                frames,
                locals,
            } => {
                let Some(top) = self.play.debug.script_of(*script) else {
                    // Not a script of this session: let it go on.
                    self.play_debug_send_resume(*side, *pause, DebugAction::Continue);
                    return true;
                };
                let context = match side {
                    Side::Server => OutputContext::Server,
                    Side::Client => OutputContext::Client,
                };
                let place = self.edit_dom();
                let mut stack = Vec::new();
                let mut scripts = Vec::new();
                // The probe leaves its own frames out; an older probe did not.
                let frames: Vec<_> = frames
                    .iter()
                    .filter(|frame| !frame.source.contains("__rbxnative_"))
                    .collect();
                for (index, frame) in frames.iter().enumerate() {
                    let found = if index == 0 {
                        Some(top)
                    } else {
                        self.play_place_script(context, &frame.source)
                    };
                    let name = if frame.function.is_empty() {
                        frame
                            .source
                            .rsplit('.')
                            .next()
                            .unwrap_or(&frame.source)
                            .to_owned()
                    } else {
                        frame.function.clone()
                    };
                    stack.push(Frame {
                        function: name,
                        line: frame.line,
                    });
                    scripts.push(found);
                }
                if stack.is_empty() {
                    let name = crate::script_editor::source::label(place, top).unwrap_or_default();
                    stack.push(Frame {
                        function: name,
                        line: *line,
                    });
                    scripts.push(Some(top));
                }
                let shown = DebugPause {
                    line: *line,
                    stack,
                    variables: locals
                        .iter()
                        .map(|v| Variable {
                            name: v.name.clone(),
                            value: v.value.clone(),
                        })
                        .collect(),
                    output: Vec::new(),
                    scripts,
                };
                let paused = PlayPause {
                    side: *side,
                    id: *pause,
                    script: top,
                };
                self.on_play_pause(paused, shown, cx);
                true
            }
            Uplink::DebugValues {
                side,
                pause,
                values,
            } => {
                let values = values
                    .iter()
                    .map(|v| {
                        let value = if v.ok {
                            Ok(v.text.clone())
                        } else {
                            Err(v.text.clone())
                        };
                        (v.expression.clone(), value)
                    })
                    .collect();
                self.on_play_values(*side, *pause, values);
                cx.notify();
                true
            }
            _ => false,
        }
    }

    fn play_debug_send_resume(&self, side: Side, pause: u64, action: DebugAction) {
        if let Some(link) = self.play.link.as_ref() {
            link.channel.send(CommandBody::DebugResume {
                side,
                pause,
                action,
            });
        }
    }

    pub(in crate::shell) fn play_debug_resume(&self, paused: PlayPause, resume: Resume) {
        self.play_debug_send_resume(paused.side, paused.id, action(resume));
    }

    pub(in crate::shell) fn play_debug_watch(&self, paused: PlayPause, expressions: Vec<String>) {
        if expressions.is_empty() {
            return;
        }
        if let Some(link) = self.play.link.as_ref() {
            link.channel.send(CommandBody::DebugWatch {
                side: paused.side,
                pause: paused.id,
                expressions,
            });
        }
    }

    /// Opens `script` (of the edited place) at `line`. A pause arrives on
    /// the Play poll, which has no `Window`, so the reveal runs in the
    /// window the test was started from.
    pub(in crate::shell) fn play_reveal_line(
        &mut self,
        script: Ref,
        line: u32,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self.play.window_handle else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |shell, cx| {
                    let at = lsp_types::Position::new(line.saturating_sub(1), 0);
                    shell.reveal(script, lsp_types::Range::new(at, at), window, cx);
                });
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    // Not `use super::*`: it would bring in GPUI's `test` attribute.
    use super::{action, DebugAction, DebugScript, PlayDebug, Ref, Resume};

    #[test]
    fn steps_map_to_probe_actions() {
        assert_eq!(action(Resume::Continue), DebugAction::Continue);
        assert_eq!(action(Resume::StepInto), DebugAction::StepInto);
        assert_eq!(action(Resume::StepOver), DebugAction::StepOver);
        assert_eq!(action(Resume::StepOut), DebugAction::StepOut);
    }

    #[test]
    fn scripts_are_found_by_id_and_by_ref() {
        let debug = PlayDebug {
            scripts: vec![DebugScript {
                id: 3,
                script: Ref::new(42),
                lines: vec![1, 4],
            }],
            ..PlayDebug::default()
        };
        assert_eq!(debug.script_of(3), Some(Ref::new(42)));
        assert_eq!(debug.script_of(4), None);
        assert_eq!(debug.id_of(Ref::new(42)).map(|s| s.id), Some(3));
    }
}
