//! Starting the platform's shaper at Play and pushing applied conditions
//! to it (`rbx_play::netsim`): Linux runs the client inside its namespace,
//! macOS and Windows shape the client's UDP ports machine-wide.

use gpui_kit::*;
use rbx_play::launch::ClientOptions;
use rbx_play::netsim::NetConditions;

use super::Choice;
use crate::command_bar::Feedback;
use crate::shell::play::session::PLAY_SOURCE;
use crate::shell::Shell;

impl Shell {
    /// Sends `conditions` to the shaper the running client is in; the
    /// reason when that failed.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(super) fn network_push(&mut self, conditions: &NetConditions) -> Option<String> {
        let shaper = self.play.network.shaper.as_mut()?;
        let error = shaper.apply(conditions).err()?;
        let text = format!("Network simulation could not change: {error}.");
        self.output
            .push(PLAY_SOURCE, Feedback::Warning(text.clone()));
        Some(text)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub(super) fn network_push(&mut self, _conditions: &NetConditions) -> Option<String> {
        None
    }

    /// The Play start's network part: with a preset applied and Sober as the
    /// launcher, the client runs inside the shaper. Anything missing leaves
    /// the launch unshaped, with the reason in Output.
    pub(in crate::shell) fn network_client_options(&mut self, options: &mut ClientOptions) {
        self.play.network.shaped_client = false;
        if self.play.network.applied.choice == Choice::Off {
            return;
        }
        match self.network_wrapper() {
            Ok(wrapper) => {
                options.wrapper = wrapper;
                self.play.network.shaped_client = true;
            }
            Err(reason) => self.output.push(
                PLAY_SOURCE,
                Feedback::Warning(format!(
                    "Network simulation is off for this test: {reason}."
                )),
            ),
        }
    }

    #[cfg(target_os = "linux")]
    fn network_wrapper(&mut self) -> Result<Vec<String>, String> {
        use rbx_play::launch::Launcher;
        use rbx_play::netsim::Shaper;
        if Launcher::detect() != Launcher::Sober {
            return Err("it needs the Sober client (flatpak), which was not found".to_owned());
        }
        let network = &mut self.play.network;
        if !network.shaper.as_mut().is_some_and(Shaper::alive) {
            // Dropped first, so its namespace is gone before the next starts.
            network.shaper = None;
            let program = crate::shell::play::slirp4netns::program()
                .map_err(|err| format!("slirp4netns could not be unpacked ({err})"))?;
            // On the UI thread on purpose: its helpers end with the thread
            // that started them (`PR_SET_PDEATHSIG`).
            network.shaper = Some(Shaper::start(&program, false).map_err(|err| err.to_string())?);
        }
        let shaper = network.shaper.as_mut().expect("started above");
        shaper
            .apply(&network.applied.conditions)
            .map_err(|err| err.to_string())?;
        Ok(shaper.wrapper())
    }

    /// macOS and Windows shape by the client's UDP ports, machine-wide, so
    /// there is no wrapper: the shaper only has to be running (and watching
    /// for the client's ports) before the client starts.
    #[cfg(any(target_os = "macos", windows))]
    fn network_wrapper(&mut self) -> Result<Vec<String>, String> {
        use rbx_play::netsim::Shaper;
        let network = &mut self.play.network;
        if !network.shaper.as_mut().is_some_and(Shaper::alive) {
            network.shaper = None;
            let shaper = Shaper::start().map_err(|err| err.to_string())?;
            if let Some(notice) = shaper.notice() {
                self.output.push(
                    PLAY_SOURCE,
                    Feedback::Warning(format!("Network simulation: {notice}.")),
                );
            }
            self.play.network.shaper = Some(shaper);
        }
        let network = &mut self.play.network;
        let shaper = network.shaper.as_mut().expect("started above");
        shaper
            .apply(&network.applied.conditions)
            .map_err(|err| err.to_string())?;
        Ok(Vec::new())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    fn network_wrapper(&mut self) -> Result<Vec<String>, String> {
        Err(unsupported().unwrap_or("not supported here").to_owned())
    }
}

/// Why this platform has no Network Simulator, for the tile's tooltip.
pub(in crate::shell) fn unsupported() -> Option<&'static str> {
    if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        None
    } else {
        Some("not on this platform: there is no packet shaper for it")
    }
}

/// What the first shaped Play needs on this platform, for the tile's
/// tooltip: both shapers need elevation, which the editor asks for once
/// and keeps until it quits.
pub(in crate::shell) fn prerequisite() -> Option<&'static str> {
    if cfg!(windows) {
        Some(
            "needs WinDivert.dll and WinDivert64.sys (WinDivert 2.2, x64) next to \
             rbxstudio.exe; the first shaped Play asks for administrator approval",
        )
    } else if cfg!(target_os = "macos") {
        Some("the first shaped Play asks for an administrator password (pf and dummynet need root)")
    } else {
        None
    }
}
