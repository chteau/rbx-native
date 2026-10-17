//! The one Output line a Play session on a Server Authority place gets.

use gpui_kit::Context;

use super::session::PLAY_SOURCE;
use crate::command_bar::Feedback;
use crate::properties::attributes::replication::is_server_authority;
use crate::shell::Shell;

const NOTE: &str = "This place uses Server Authority. Prediction, rollback and resimulation run \
     in the real Roblox client and server, so what you see is the real engine's behaviour. The \
     Network Simulator shapes the real UDP traffic, so latency handling can be tested here.";

impl Shell {
    /// Explains, once per Play start, why a Server Authority place behaves
    /// as it does under the sandbox.
    pub(in crate::shell) fn note_server_authority(&mut self, cx: &mut Context<Self>) {
        if is_server_authority(&self.dom) {
            self.output.push(PLAY_SOURCE, Feedback::Info(NOTE.into()));
            cx.notify();
        }
    }
}
