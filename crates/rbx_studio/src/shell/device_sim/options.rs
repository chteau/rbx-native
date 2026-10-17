//! The Device menu's options for the picked device: the on-screen keyboard,
//! the Android back button and Display scaling.

use gpui_kit::*;

use super::super::menu;
use super::super::Shell;
use crate::device_sim::DisplayScaling;

impl Shell {
    pub(super) fn device_option_items(&self, cx: &App) -> Vec<menu::Item> {
        let device = self.simulated_device(cx);
        let orientation = self.device_sim.orientation;
        let has_keyboard = device.is_some_and(|d| d.keyboard_height(orientation) > 0);
        let keyboard = menu::item("On-screen keyboard")
            .checked(has_keyboard && self.device_sim.keyboard)
            .hint(
                "Covers the bottom of the screen like the system keyboard, and in Play also \
                 while a TextBox is focused. Drawn by the editor only: the game is not told \
                 (OnScreenKeyboardSize stays 0).",
            );
        let back_hint = if cfg!(target_os = "linux") {
            "Android devices, during Play: sends Escape to the client, which opens \
             Roblox\u{2019}s menu as Back does."
        } else {
            "Linux only: the client takes the key through Sober\u{2019}s window."
        };
        let back = menu::item("Android back button").hint(back_hint);
        let mut items = vec![
            match has_keyboard {
                true => keyboard.on_click(|shell, cx| {
                    shell.device_sim.keyboard = !shell.device_sim.keyboard;
                    shell.refresh_device_view(cx);
                }),
                false => keyboard.disabled(),
            },
            match self.back_button_ready(cx) {
                true => back.on_click(|shell, _| shell.press_back()),
                false => back.disabled(),
            },
            menu::separator(),
            menu::item("Display scaling").disabled(),
        ];
        for scaling in DisplayScaling::ALL {
            items.push(
                menu::item(scaling.label())
                    .checked(self.device_sim.scaling == scaling)
                    .hint(scaling_hint(scaling))
                    .on_click(move |shell, cx| {
                        shell.device_sim.scaling = scaling;
                        shell.save_settings();
                        shell.refresh_device_view(cx);
                    }),
            );
        }
        items
    }
}

fn scaling_hint(scaling: DisplayScaling) -> &'static str {
    match scaling {
        DisplayScaling::Fit => "As large as the 3D view allows.",
        DisplayScaling::Actual => {
            "One device point to one editor pixel, the size Play shows the client at."
        }
        DisplayScaling::Physical => {
            "The device\u{2019}s real size, assuming 96 dpi \u{d7} the window\u{2019}s scale. \
             Monitors, TVs and headsets have no size: fit instead. Play is always 1:1."
        }
    }
}
