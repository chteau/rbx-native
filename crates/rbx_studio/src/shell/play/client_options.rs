//! What the playtest tools ask of the client a Play session launches.

use gpui_kit::Context;
use rbx_play::launch::ClientOptions;

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
        options
    }
}
