//! The Device Simulator: the Test ribbon's Device menu, and what a picked
//! device does to the 3D view and to the next Play.
//!
//! A game script cannot change what the client reports (`ViewportSize`,
//! `TouchEnabled`, `IsTenFootInterface` are read-only, and the emulation
//! members are plugin-only), so nothing here fakes a value. In Edit the view
//! is our own renderer: its `ScreenGui`s are laid out at the device's
//! logical size and safe area. In Play the real client is sized to the
//! device's logical resolution (see `workspace_view::device`), and Sober is
//! launched in the device's input mode (`device_sim::sober_keys`), so the
//! client reports genuine values. Pixel density cannot be simulated either
//! way: Sober's `enable_hidpi` scales the window, it does not report a DPI.
//!
//! `RBX_STUDIO_DEVICE=<device name>[,rotate]` picks a device at startup and
//! shows the Test ribbon — a screenshot aid, since nothing else can open the
//! Device menu on the editor's behalf (see `AGENTS.md`'s safety rules).

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_viewer::SafeInsets;

use super::menu::{self, MenuId};
use super::play::session::PLAY_SOURCE;
use super::play::Phase;
use super::Shell;
use crate::command_bar::Feedback;
use crate::device_sim::{self, CustomDevice, Device, Orientation, Platform, Tier};
use crate::tokens;
use crate::workspace_view::device::Simulated;

/// Which device the menu has picked: one of `device_sim::devices`, or the
/// custom one from settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Choice {
    Builtin(usize),
    Custom,
}

pub(super) struct DeviceSim {
    choice: Option<Choice>,
    orientation: Orientation,
    /// Persisted in settings as `device_custom`.
    pub(super) custom: CustomDevice,
    /// The platform the running Play client was launched for (`None` inside:
    /// launched without simulation), `None` while no client runs. Sober reads
    /// its input mode at launch only, so a change of class needs a restart.
    launched: Option<Option<Platform>>,
}

impl DeviceSim {
    pub(super) fn new(custom: CustomDevice) -> Self {
        DeviceSim {
            choice: None,
            orientation: Orientation::Landscape,
            custom,
            launched: None,
        }
    }

    fn device(&self) -> Option<Device> {
        match self.choice? {
            Choice::Builtin(index) => device_sim::devices().get(index).copied(),
            Choice::Custom => Some(self.custom.device()),
        }
    }
}

/// Read once at startup by `Shell::new`; documented in this module's doc.
pub(crate) const DEVICE_VARIABLE: &str = "RBX_STUDIO_DEVICE";

impl Shell {
    /// [`DEVICE_VARIABLE`]'s startup pick.
    pub(super) fn apply_debug_device(&mut self, spec: &str, cx: &mut Context<Self>) {
        let mut parts = spec.split(',').map(str::trim);
        let name = parts.next().unwrap_or_default();
        if let Some(index) = device_sim::devices()
            .iter()
            .position(|device| device.name.eq_ignore_ascii_case(name))
        {
            self.pick_device(Some(Choice::Builtin(index)), cx);
            if parts.any(|part| part == "rotate") {
                self.rotate_device(cx);
            }
        }
        self.ribbon_tab = super::ribbon::Tab::Test;
    }

    /// The picked device, while the 3D view still shows its screen: a size
    /// typed into the Screen row or the UI Editor replaces it.
    fn simulated_device(&self, cx: &App) -> Option<Device> {
        let device = self.device_sim.device()?;
        let screen = device.screen(self.device_sim.orientation);
        (self.viewport.read(cx).emulated_screen() == Some(screen)).then_some(device)
    }

    pub(super) fn pick_device(&mut self, choice: Option<Choice>, cx: &mut Context<Self>) {
        self.device_sim.choice = choice;
        if let Some(device) = self.device_sim.device() {
            self.device_sim.orientation = device.default_orientation();
        }
        self.apply_device(cx);
    }

    fn rotate_device(&mut self, cx: &mut Context<Self>) {
        self.device_sim.orientation = self.device_sim.orientation.rotated();
        self.apply_device(cx);
    }

    /// Puts the device's screen on the view: the same resolution the UI
    /// Editor lays out at, so the canvas and the 3D view agree.
    fn apply_device(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.device_sim.device() else {
            self.clear_viewport_screen(cx);
            self.device_restart_note(None);
            return;
        };
        let orientation = self.device_sim.orientation;
        self.set_resolution(device.screen(orientation), cx);
        let insets = device.insets(orientation);
        let simulated = Simulated {
            label: device.label(orientation).into(),
            insets: SafeInsets {
                left: insets.left as f32,
                top: insets.top as f32,
                right: insets.right as f32,
                bottom: insets.bottom as f32,
            },
            plays: device.platform.plays(),
        };
        self.viewport
            .update(cx, |view, cx| view.set_device(Some(simulated), cx));
        self.device_restart_note(Some(device.platform));
    }

    /// Size and orientation follow live; the input mode does not.
    fn device_restart_note(&mut self, now: Option<Platform>) {
        let Some(launched) = self.device_sim.launched else {
            return;
        };
        if cfg!(target_os = "linux") && device_sim::needs_restart(launched, now) {
            self.output.push(
                PLAY_SOURCE,
                Feedback::Warning(
                    "Stop and Play again to switch the client to this device\u{2019}s input \
                     (touch, console or desktop): Sober reads it at launch only."
                        .to_owned(),
                ),
            );
        }
    }

    /// The Sober config keys for the next Play's client: the simulated
    /// device's input mode, or none (the user's own config) without one.
    pub(super) fn device_sober_config(&mut self, cx: &App) -> Vec<(String, serde_json::Value)> {
        let platform = self.simulated_device(cx).map(|device| device.platform);
        self.device_sim.launched = Some(platform);
        platform.map(device_sim::sober_keys).unwrap_or_default()
    }

    /// Tells the view whether the client is embedded over it, and forgets
    /// the launch's input mode once it stops.
    pub(super) fn device_play_changed(&mut self, cx: &mut Context<Self>) {
        let playing = self.play.phase == Phase::Running;
        if self.play.phase == Phase::Idle {
            self.device_sim.launched = None;
        }
        self.viewport
            .update(cx, |view, cx| view.set_playing(playing, cx));
    }

    fn set_custom_device(&mut self, custom: CustomDevice, cx: &mut Context<Self>) {
        self.device_sim.custom = custom;
        self.save_settings();
        self.pick_device(Some(Choice::Custom), cx);
    }

    /// The Test ribbon's Device tile and its menu.
    pub(super) fn device_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.simulated_device(cx).is_some();
        let tile = super::ribbon::tile(
            &self.ribbon_nav,
            "ribbon-device",
            IconName::Smartphone,
            "Device",
            cx,
        )
        .when(active, |this| {
            this.bg(tokens::accent_soft())
                .text_color(tokens::check_on())
        });
        menu::dropdown(
            self,
            MenuId::Device,
            super::chrome::Trigger::new(tile),
            self.device_items(cx),
            cx,
        )
        .into_any_element()
    }

    fn device_items(&self, cx: &App) -> Vec<menu::Item> {
        let current = self.simulated_device(cx).and(self.device_sim.choice);
        let mut items = vec![menu::item("Current device")
            .hint("No simulation: the 3D view and the Play client at their own size and input.")
            .checked(current.is_none())
            .on_click(|shell, cx| shell.pick_device(None, cx))];
        // Near the top, where it is reached for most, not below a long list.
        let rotates = current.is_some()
            && self
                .device_sim
                .device()
                .is_some_and(|device| device.platform.rotates());
        let rotate = menu::item(format!(
            "Rotate to {}",
            self.device_sim.orientation.rotated().label()
        ))
        .icon(IconName::RotateCw);
        items.push(match rotates {
            true => rotate.on_click(|shell, cx| shell.rotate_device(cx)),
            false => rotate.disabled(),
        });
        let mut group = None;
        for (index, device) in device_sim::devices().iter().enumerate() {
            let heading = (device.platform, device.tier);
            if group != Some(heading) {
                group = Some(heading);
                items.push(menu::separator());
                items.push(menu::item(group_label(device.platform, device.tier)).disabled());
            }
            let (w, h) = device.screen(device.default_orientation());
            let mut item = menu::item(format!("{}  {w}\u{d7}{h}", device.name))
                .checked(current == Some(Choice::Builtin(index)))
                .on_click(move |shell, cx| shell.pick_device(Some(Choice::Builtin(index)), cx));
            if !device.platform.plays() {
                item =
                    item.hint("Edit preview only. Play: not available \u{2014} Sober has no VR.");
            }
            items.push(item);
        }
        items.extend(self.custom_items(current, cx));
        items
    }

    /// The custom device: picked as is, its size taken from the screen the
    /// view shows (typed into the Viewport dock's Screen row), and its
    /// platform cycled through the five.
    fn custom_items(&self, current: Option<Choice>, cx: &App) -> Vec<menu::Item> {
        let custom = self.device_sim.custom;
        let mut items = vec![
            menu::separator(),
            menu::item("Custom").disabled(),
            menu::item(format!(
                "{}\u{d7}{} {}",
                custom.width,
                custom.height,
                custom.platform.label()
            ))
            .checked(current == Some(Choice::Custom))
            .on_click(|shell, cx| shell.pick_device(Some(Choice::Custom), cx)),
        ];
        let screen = self.viewport.read(cx).emulated_screen();
        let take = menu::item("Use the Screen size as Custom")
            .hint("Type a width and height in the Viewport dock's Screen row first.");
        items.push(match screen {
            Some((width, height)) => take.on_click(move |shell, cx| {
                let custom = shell.device_sim.custom;
                shell.set_custom_device(CustomDevice::new(width, height, custom.platform), cx);
            }),
            None => take.disabled(),
        });
        // One row cycling the platform keeps the menu within a window's
        // height.
        let at = Platform::ALL.iter().position(|&p| p == custom.platform);
        let next = Platform::ALL[at.map_or(0, |at| (at + 1) % Platform::ALL.len())];
        items.push(
            menu::item(format!(
                "Custom platform: {} \u{2192} {}",
                custom.platform.label(),
                next.label()
            ))
            .on_click(move |shell, cx| {
                let custom = shell.device_sim.custom;
                shell.set_custom_device(CustomDevice::new(custom.width, custom.height, next), cx);
            }),
        );
        items
    }
}

fn group_label(platform: Platform, tier: Option<Tier>) -> String {
    match tier {
        Some(tier) => format!("{} \u{b7} {}", platform.label(), tier.label()),
        None => platform.label().to_owned(),
    }
}
