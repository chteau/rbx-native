//! The Play half of the Device Simulator: the client's self-report and the
//! Android back button.

use gpui_kit::*;
use rbx_play::protocol::DeviceReport;

use super::super::play::session::PLAY_SOURCE;
use super::super::Shell;
use crate::command_bar::Feedback;
use crate::device_sim::{self, Os};
use crate::workspace_view::device::KeyTap;

impl Shell {
    /// An `Uplink::Device`: kept for the badge and the keyboard cover; the
    /// first of each Play also goes to Output, so a live test leaves a record.
    pub(in crate::shell) fn device_client_report(
        &mut self,
        report: DeviceReport,
        cx: &mut Context<Self>,
    ) {
        if self.device_sim.client.is_none() {
            let orientation = self.device_sim.orientation;
            let screen = self
                .simulated_device(cx)
                .map(|device| device.screen(orientation));
            self.output.push(
                PLAY_SOURCE,
                Feedback::Info(device_sim::report::output_line(&report, screen)),
            );
        }
        self.device_sim.client = Some(report);
        self.refresh_device_view(cx);
    }

    /// Tells the running client where the keyboard cover is, `height` tall
    /// over a `screen`-sized client (0 while it is down), when that changed.
    pub(super) fn device_keyboard_to_client(&mut self, screen: (u32, u32), height: u32) {
        if self.device_sim.launched.is_none() {
            return;
        }
        let rect = device_sim::report::keyboard_rect(screen, height);
        self.play_keep_focus_for_cover(rect);
        if self.device_sim.keyboard_sent == Some(rect) {
            return;
        }
        let [x, y, width, height] = rect;
        let body = rbx_play::protocol::CommandBody::Keyboard {
            x,
            y,
            width,
            height,
        };
        if self.play_send(body).is_some() {
            self.device_sim.keyboard_sent = Some(rect);
        }
    }

    /// Studio's rule: an Android device, while the client view is active.
    pub(super) fn back_button_ready(&self, cx: &App) -> bool {
        let android = self
            .simulated_device(cx)
            .is_some_and(|device| device.os == Os::Android);
        #[cfg(target_os = "linux")]
        let shown = self.play_embedded().is_some();
        #[cfg(not(target_os = "linux"))]
        let shown = false;
        android && shown
    }

    /// Escape into the embedded client: what Back does in a running
    /// Android game is open Roblox's menu, which Escape opens too. Which key
    /// Sober itself maps to Android's Back is not documented.
    /// A key typed on the keyboard cover, into the embedded client: nothing
    /// outside Play, where no client takes it.
    pub(in crate::shell) fn type_into_client(&mut self, tap: &KeyTap) {
        #[cfg(target_os = "linux")]
        {
            use rbx_play::embed::Key;
            let keys: Vec<Key> = match tap {
                // A variation selector has no key; the character alone
                // still types the emoji.
                KeyTap::Text(text) => text
                    .chars()
                    .filter(|&c| c != '\u{fe0f}')
                    .map(Key::Char)
                    .collect(),
                KeyTap::Backspace => vec![Key::Backspace],
                KeyTap::Return => vec![Key::Return],
            };
            if let Some(Err(e)) = self.play_embedded().map(|client| client.type_keys(&keys)) {
                self.output.push(
                    PLAY_SOURCE,
                    Feedback::Warning(format!("Keyboard cover: {e}")),
                );
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = tap;
    }

    pub(super) fn press_back(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(Err(e)) = self.play_embedded().map(|client| client.press_escape()) {
            self.output.push(
                PLAY_SOURCE,
                Feedback::Warning(format!("Android back button: {e}")),
            );
        }
    }
}
