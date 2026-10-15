//! Plugins driving the script debugger (`plugin:GetDebuggerManager()`).

use gpui_kit::*;
use rbx_lua::plugin::{DebuggerRequest, DebuggerView};

use crate::shell::Shell;

impl Shell {
    /// The debugger as plugins see it (`Context::debugger`).
    pub(super) fn plugin_debugger_view(&self) -> DebuggerView {
        todo!()
    }

    /// Carries out a plugin's `Request::Debugger` through the same paths
    /// the editor's own controls use.
    pub(super) fn plugin_debugger_request(&mut self, request: DebuggerRequest, cx: &mut Context<Self>) {
        todo!()
    }

    /// Called when the debugger paused, resumed, finished, or breakpoints/
    /// watches changed: `Input::DebuggerChanged` when the view differs from
    /// the one plugins last saw.
    pub(in crate::shell) fn plugins_debugger_changed(&mut self, cx: &mut Context<Self>) {
        todo!()
    }
}
