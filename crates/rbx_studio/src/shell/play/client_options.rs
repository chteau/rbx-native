//! What the playtest tools ask of the client a Play session launches.

use gpui_kit::Context;
use rbx_play::launch::{ClientOptions, Launcher};

use crate::command_bar::Feedback;
use crate::shell::play::session::PLAY_SOURCE;
use crate::shell::Shell;

impl Shell {
    /// The launch options for the next Play session's client: Sober config
    /// keys, `flatpak run` arguments and a wrapper, gathered from the tools
    /// that shape the client (device, network, controller).
    pub(in crate::shell) fn play_client_options(
        &mut self,
        launcher: Launcher,
        cx: &mut Context<Self>,
    ) -> ClientOptions {
        let mut options = ClientOptions {
            sober_config: self.device_sober_config(cx),
            ..ClientOptions::default()
        };
        self.network_client_options(&mut options);
        self.controller_client_options(&mut options, cx);
        self.quiet_client_notice(launcher);
        self.flatpak_client_notice(&options);
        self.sober_only_notice(launcher, &options);
        options
    }

    /// Another client gets the simulated device through the probe and the
    /// controller through `--device=input`, but only Sober can be started
    /// as a phone or a console, so Roblox's own menus stay desktop ones.
    pub(in crate::shell) fn sober_only_notice(
        &mut self,
        launcher: Launcher,
        options: &ClientOptions,
    ) {
        if launcher == Launcher::Sober {
            return;
        }
        let key = |name: &str| {
            options
                .sober_config
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
        };
        let device = if key("touch_mode").is_some_and(|value| value == "on") {
            "touch device"
        } else if key("use_console_experience").is_some_and(|value| value == true) {
            "console"
        } else {
            return;
        };
        self.output.push(
            PLAY_SOURCE,
            Feedback::Output(format!(
                "{} runs Roblox's desktop client, so Roblox's own menus stay desktop ones. \
                 Your scripts still see a {device}.",
                launcher.name()
            )),
        );
    }

    /// In the Flatpak build Sober is opened through the desktop portal, so
    /// the options that shape the client can't reach it; say so.
    fn flatpak_client_notice(&mut self, options: &ClientOptions) {
        if rbx_play::sandbox::is_flatpak()
            && (!options.sober_config.is_empty() || !options.flatpak_args.is_empty())
        {
            self.output.push(
                PLAY_SOURCE,
                Feedback::Warning(
                    "The Flatpak build opens Sober through the desktop portal, so the simulated \
                     device and the controller emulator don't reach this test. The GitHub build \
                     of rbx-native has them."
                        .to_owned(),
                ),
            );
        }
    }

    /// Sober and Roséline keep Roblox's own volume slider between runs; a
    /// low one makes Play sound silent although the client's stream plays,
    /// so say so.
    pub(in crate::shell) fn quiet_client_notice(&mut self, launcher: Launcher) {
        if !cfg!(target_os = "linux") {
            return;
        }
        let notice = rbx_play::sober_config::master_volume(launcher)
            .and_then(rbx_play::sober_config::quiet_volume_notice);
        if let Some(text) = notice {
            self.output.push(PLAY_SOURCE, Feedback::Warning(text));
        }
    }
}
