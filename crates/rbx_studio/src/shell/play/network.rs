//! The Network Simulator: the Test ribbon's Network pill and its popover,
//! shaped like Studio's (`studio/network-simulator.md`). Picking a preset or
//! editing a value only stages it; Apply makes it the applied setting and,
//! mid-test, changes the running client's network at once.
//!
//! What it shapes, honestly: the Roblox client's UDP game traffic
//! (`rbx_play::netsim`), so the game itself lags, as in Studio. Not the
//! client's TCP (asset downloads stay fast, also as in Studio), and not this
//! editor's live view, which reaches the editor from the game server
//! through the tunnel.
//!
//! One deliberate difference: Studio's default is Ideal Fiber (8 ms each
//! way), but its playtest server is local. Here the server is a real
//! Roblox server across the internet, so the default is **Off**: Play
//! launches the client as it always did, and only an applied preset puts
//! it in the shaped network. Applied values last until the editor quits;
//! saved presets are kept in settings.
//!
//! `RBX_STUDIO_NETWORK=<preset>[,open]` applies a built-in preset (by its
//! pill or its name) at startup and shows the Test ribbon, with the popover
//! open under `,open` — a screenshot aid, since nothing else can click the
//! ribbon on the editor's behalf (see `AGENTS.md`'s safety rules).

mod menu;
mod popover;
mod shaper;

use gpui_kit::*;
use rbx_play::netsim::{is_reserved_name, NetConditions, OneWay, PRESETS};

use crate::settings::SavedNetworkPreset;
use crate::shell::Shell;

pub(in crate::shell) use popover::Popover;
use shaper::{prerequisite, unsupported};

/// What the Preset menu shows as chosen.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) enum Choice {
    /// No simulation: the client launches outside the shaped network.
    Off,
    BuiltIn(usize),
    Saved(String),
    Custom,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct Setting {
    pub(in crate::shell) choice: Choice,
    pub(in crate::shell) conditions: NetConditions,
}

impl Setting {
    const OFF: Setting = Setting {
        choice: Choice::Off,
        conditions: NetConditions {
            inbound: OneWay::new(0.0, 0.0, 0.0),
            outbound: OneWay::new(0.0, 0.0, 0.0),
        },
    };

    fn built_in(index: usize) -> Setting {
        Setting {
            choice: Choice::BuiltIn(index),
            conditions: PRESETS[index].conditions,
        }
    }

    /// The pill's short label: the connection type, as Studio's shows.
    pub(in crate::shell) fn pill(&self) -> &'static str {
        match self.choice {
            Choice::Off => "Off",
            Choice::BuiltIn(index) => PRESETS[index].pill,
            Choice::Saved(_) | Choice::Custom => "Custom",
        }
    }

    pub(in crate::shell) fn name(&self) -> String {
        match &self.choice {
            Choice::Off => "Off".to_owned(),
            Choice::BuiltIn(index) => PRESETS[*index].name.to_owned(),
            Choice::Saved(name) => name.clone(),
            Choice::Custom => "Custom".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Way {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Field {
    Latency,
    Jitter,
    Loss,
}

impl Field {
    pub(in crate::shell) fn get(self, way: OneWay) -> f64 {
        match self {
            Field::Latency => way.latency_ms,
            Field::Jitter => way.jitter_ms,
            Field::Loss => way.loss_percent,
        }
    }

    fn set(self, way: &mut OneWay, value: f64) {
        match self {
            Field::Latency => way.latency_ms = value,
            Field::Jitter => way.jitter_ms = value,
            Field::Loss => way.loss_percent = value,
        }
    }
}

fn way_mut(conditions: &mut NetConditions, way: Way) -> &mut OneWay {
    match way {
        Way::Inbound => &mut conditions.inbound,
        Way::Outbound => &mut conditions.outbound,
    }
}

pub(in crate::shell) struct NetworkSim {
    pub(in crate::shell) saved: Vec<SavedNetworkPreset>,
    pub(in crate::shell) applied: Setting,
    /// The popover's edits while it is open; dismissing it drops them.
    pub(in crate::shell) staged: Option<Setting>,
    /// The running client was launched inside the shaper, so Apply reaches
    /// it; otherwise an applied preset waits for the next Play.
    shaped_client: bool,
    /// Started by the first shaped Play and kept for later ones, like the
    /// tunnel; ended when the editor quits.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    shaper: Option<rbx_play::netsim::Shaper>,
    pub(in crate::shell) popover: Option<Popover>,
    /// When the popover last closed, so the tile click that closed it from
    /// outside does not reopen it.
    closed_at: Option<std::time::Instant>,
}

impl NetworkSim {
    pub(in crate::shell) fn new(saved: Vec<SavedNetworkPreset>) -> Self {
        NetworkSim {
            saved,
            applied: Setting::OFF,
            staged: None,
            shaped_client: false,
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            shaper: None,
            popover: None,
            closed_at: None,
        }
    }

    /// What the pill and the popover show: the staged edits while open.
    pub(in crate::shell) fn shown(&self) -> &Setting {
        self.staged.as_ref().unwrap_or(&self.applied)
    }

    fn staged_mut(&mut self) -> &mut Setting {
        self.staged.get_or_insert_with(|| self.applied.clone())
    }

    /// Stages a menu entry: Off, a built-in, or a saved preset by name.
    pub(in crate::shell) fn stage(&mut self, choice: Choice) {
        let setting = match &choice {
            Choice::Off | Choice::Custom => Setting::OFF,
            Choice::BuiltIn(index) => Setting::built_in(*index),
            Choice::Saved(name) => match self.saved.iter().find(|p| &p.name == name) {
                Some(preset) => Setting {
                    choice: choice.clone(),
                    conditions: preset.conditions,
                },
                None => return,
            },
        };
        *self.staged_mut() = setting;
    }

    /// A typed value; any change makes the staged setting Custom. Returns
    /// whether it changed anything (a field re-showing its own value does
    /// not).
    pub(in crate::shell) fn edit(&mut self, way: Way, field: Field, value: f64) -> bool {
        let staged = self.staged_mut();
        let mut next = staged.conditions;
        field.set(way_mut(&mut next, way), value);
        let next = next.clamped();
        if next == staged.conditions {
            return false;
        }
        staged.conditions = next;
        staged.choice = Choice::Custom;
        true
    }

    /// Studio's Reset: stages Ideal Fiber, which still needs Apply.
    pub(in crate::shell) fn reset(&mut self) {
        *self.staged_mut() = Setting::built_in(0);
    }

    /// Why `name` cannot be saved, or whether it would replace one.
    pub(in crate::shell) fn check_name(&self, name: &str) -> Result<bool, &'static str> {
        let name = name.trim();
        if name.is_empty() {
            Err("Give the preset a name.")
        } else if is_reserved_name(name) {
            Err("Built-in preset names and \u{201c}Custom\u{201d} are reserved.")
        } else {
            Ok(self.saved.iter().any(|p| p.name == name))
        }
    }

    /// Saves the staged values under `name` (replacing a preset of that
    /// name) and stages that preset; the caller applies it.
    pub(in crate::shell) fn save(&mut self, name: &str) -> Result<(), &'static str> {
        self.check_name(name)?;
        let name = name.trim().to_owned();
        let conditions = self.shown().conditions;
        match self.saved.iter_mut().find(|p| p.name == name) {
            Some(preset) => preset.conditions = conditions,
            None => self.saved.push(SavedNetworkPreset {
                name: name.clone(),
                conditions,
            }),
        }
        self.staged_mut().choice = Choice::Saved(name);
        Ok(())
    }

    /// Studio's Delete: removes the staged saved preset and stages Ideal
    /// Fiber, which the caller applies at once.
    pub(in crate::shell) fn delete(&mut self) -> bool {
        let Choice::Saved(name) = &self.shown().choice else {
            return false;
        };
        let name = name.clone();
        self.saved.retain(|p| p.name != name);
        self.reset();
        true
    }

    /// Makes the staged setting the applied one; the popover stays open on
    /// it.
    fn commit(&mut self) {
        if let Some(staged) = &self.staged {
            self.applied = staged.clone();
        }
    }
}

/// Read once at startup by `Shell::new`; documented in this module's doc.
pub(crate) const NETWORK_VARIABLE: &str = "RBX_STUDIO_NETWORK";

impl Shell {
    /// [`NETWORK_VARIABLE`]'s startup edit.
    pub(in crate::shell) fn apply_debug_network(
        &mut self,
        spec: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (name, open) = match spec.split_once(',') {
            Some((name, flag)) => (name.trim(), flag.trim() == "open"),
            None => (spec.trim(), false),
        };
        if let Some(index) = PRESETS.iter().position(|preset| {
            preset.pill.eq_ignore_ascii_case(name) || preset.name.eq_ignore_ascii_case(name)
        }) {
            self.play.network.stage(Choice::BuiltIn(index));
            self.network_apply(cx);
        }
        self.ribbon_tab = crate::shell::ribbon::Tab::Test;
        if open {
            self.network_open(window, cx);
        }
    }

    /// Apply: the staged values become the applied ones and, when the
    /// running client is in the shaper, take effect on it at once.
    pub(in crate::shell) fn network_apply(&mut self, cx: &mut Context<Self>) {
        self.play.network.commit();
        let applied = self.play.network.applied.clone();
        let status = if !self.play.network.shaped_client {
            (self.play.session.is_some() && applied.choice != Choice::Off).then(|| {
                "Applies at the next Play: this test started without simulation.".to_owned()
            })
        } else {
            self.network_push(&applied.conditions)
        };
        if let Some(popover) = &mut self.play.network.popover {
            popover.status = status;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
