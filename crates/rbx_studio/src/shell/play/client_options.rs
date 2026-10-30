//! What the playtest tools ask of the client a Play session launches.

use gpui_kit::Context;
use rbx_play::launch::ClientOptions;

use crate::command_bar::Feedback;
use crate::shell::play::session::PLAY_SOURCE;
use crate::shell::Shell;

impl Shell {
    /// The launch options for the next Play session's client: Sober config
    /// keys, `flatpak run` arguments and a wrapper, gathered from the tools
    /// that shape the client (device, network, controller).
    pub(in crate::shell) fn play_client_options(
        &mut self,
        cx: &mut Context<Self>,
    ) -> ClientOptions {
        let mut options = ClientOptions {
            sober_config: self.device_sober_config(cx),
            ..ClientOptions::default()
        };
        self.network_client_options(&mut options);
        self.controller_client_options(&mut options, cx);
        self.quiet_client_notice();
        options
    }

    /// Sober keeps Roblox's own volume slider between runs; a low one makes
    /// Play sound silent although the client's stream plays, so say so.
    fn quiet_client_notice(&mut self) {
        if !cfg!(target_os = "linux") {
            return;
        }
        let notice = rbx_play::sober_config::master_volume()
            .and_then(rbx_play::sober_config::quiet_volume_notice);
        if let Some(text) = notice {
            self.output.push(PLAY_SOURCE, Feedback::Warning(text));
        }
    }
}
