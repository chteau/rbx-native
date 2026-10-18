//! The Device menu's options for the picked device: the on-screen keyboard
//! and its layout, the Android back button and Display scaling.

use gpui_kit::*;

use super::super::menu;
use super::super::Shell;
use crate::device_sim::{DisplayScaling, KeyboardLayout};

impl Shell {
    pub(super) fn device_option_items(&self, cx: &App) -> Vec<menu::Item> {
        let device = self.simulated_device(cx);
        let orientation = self.device_sim.orientation;
        let has_keyboard = device.is_some_and(|d| d.keyboard_height(orientation) > 0);
        let keyboard = menu::item("On-screen keyboard")
            .checked(has_keyboard && self.device_sim.keyboard)
            .hint(
                "Previews the system keyboard over the bottom of the screen. In Play it \
                 rises on its own while a TextBox or the chat bar has focus. Drawn by the \
                 editor only: the game is not told (OnScreenKeyboardSize stays 0).",
            );
        let back_hint = if cfg!(target_os = "linux") {
            "Android devices, during Play: sends Escape to the client, which opens \
             Roblox\u{2019}s menu as Back does."
        } else {
            "Linux only: the client takes the key through Sober\u{2019}s window."
        };
        let back = menu::item("Android back button").hint(back_hint);
        let layouts = KeyboardLayout::ALL.into_iter().map(|layout| {
            let item = menu::item(layout.label()).checked(self.device_sim.layout == layout);
            match has_keyboard {
                true => item.on_click(move |shell, cx| shell.set_keyboard_layout(layout, cx)),
                false => item.disabled(),
            }
        });
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
            menu::item("Keyboard layout").disabled(),
        ];
        items.extend(layouts);
        items.push(menu::separator());
        items.push(menu::item("Display scaling").disabled());
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
