//! Plugins driving the script debugger (`plugin:GetDebuggerManager()`).

use gpui_kit::*;
use rbx_lua::plugin::{DebuggerRequest, DebuggerView, Input};
use rbx_lua::Resume;

use crate::debugger::{Kind, Stored};
use crate::shell::Shell;

impl Shell {
    /// The debugger as plugins see it (`Context::debugger`).
    pub(super) fn plugin_debugger_view(&self) -> DebuggerView {
        self.debugger_view()
    }

    /// Carries out a plugin's `Request::Debugger` through the same paths
    /// the editor's own controls use.
    pub(super) fn plugin_debugger_request(&mut self, request: DebuggerRequest, cx: &mut Context<Self>) {
        match request {
            DebuggerRequest::SetBreakpoint {
                script,
                line,
                condition,
            } => {
                let mut stored = Stored::new(line, Kind::Standard);
                stored.breakpoint.condition = condition;
                self.debug.breakpoints.set(script, stored);
                self.sync_gutters(cx);
                self.plugins_debugger_changed(cx);
            }
            DebuggerRequest::RemoveBreakpoint { script, line } => {
                self.debug.breakpoints.remove(script, line);
                self.sync_gutters(cx);
                self.plugins_debugger_changed(cx);
            }
            DebuggerRequest::AddWatch(expression) => {
                self.debug_add_watch(expression);
                self.plugins_debugger_changed(cx);
            }
            DebuggerRequest::RemoveWatch(expression) => {
                self.debug_remove_watch(&expression);
                self.plugins_debugger_changed(cx);
            }
            DebuggerRequest::Run { script } => {
                // Opening a tab needs a window; run once it is at hand.
                let shell = cx.entity();
                cx.defer(move |cx| {
                    let Some(window) = cx.active_window() else {
                        return;
                    };
                    let _ = window.update(cx, |_, window, cx| {
                        shell.update(cx, |shell, cx| {
                            shell.open_script(script, window, cx);
                            shell.start_debugging(cx);
                        });
                    });
                });
            }
            DebuggerRequest::Resume => self.resume_debugging(Resume::Continue, cx),
            DebuggerRequest::StepIn => self.resume_debugging(Resume::StepInto, cx),
            DebuggerRequest::StepOver => self.resume_debugging(Resume::StepOver, cx),
            DebuggerRequest::StepOut => self.resume_debugging(Resume::StepOut, cx),
            DebuggerRequest::Stop => self.stop_debugging(cx),
        }
    }

    /// Called when the debugger paused, resumed, finished, or breakpoints/
    /// watches changed: `Input::DebuggerChanged` when the view differs from
    /// the one plugins last saw.
    pub(in crate::shell) fn plugins_debugger_changed(&mut self, cx: &mut Context<Self>) {
        self.plugin_input(Input::DebuggerChanged, cx);
    }
}
