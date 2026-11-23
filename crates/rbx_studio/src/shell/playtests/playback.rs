//! The dock's actions: playing the timeline, showing, renaming and deleting
//! recordings.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::super::Shell;
use crate::playtests::heat::HeatKind;
use crate::playtests::store;

/// How often a playing timeline advances.
const TICK: Duration = Duration::from_millis(33);

pub(super) use crate::playtests::clock;

impl Shell {
    pub(super) fn playtests_toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.playtests.playing {
            self.playtests.playing = false;
            self.playtests.ticker = None;
            cx.notify();
            return;
        }
        let longest = self.playtests.longest();
        if longest <= 0.0 {
            return;
        }
        if self.playtests.time >= longest {
            self.playtests.time = 0.0;
        }
        self.playtests.playing = true;
        self.playtests.ticker = Some(cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(TICK).await;
            let going = shell
                .update(cx, |shell, cx| shell.playtests_tick(TICK.as_secs_f32(), cx))
                .unwrap_or(false);
            if !going {
                break;
            }
        }));
        cx.notify();
    }

    /// One step of playback; false once it has stopped.
    fn playtests_tick(&mut self, dt: f32, cx: &mut Context<Self>) -> bool {
        if !self.playtests.playing {
            return false;
        }
        let longest = self.playtests.longest();
        let next = self.playtests.time + dt * self.playtests.speed;
        self.playtests.time = next.min(longest);
        if next >= longest {
            self.playtests.playing = false;
            self.playtests.ticker = None;
        }
        self.playtests_redraw(cx);
        self.playtests.playing
    }

    pub(super) fn playtests_set_time(&mut self, time: f32, cx: &mut Context<Self>) {
        self.playtests.time = time.clamp(0.0, self.playtests.longest());
        self.playtests_redraw(cx);
    }

    pub(super) fn playtests_set_visible(
        &mut self,
        i: usize,
        visible: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.playtests.entries.get_mut(i) else {
            return;
        };
        entry.visible = visible;
        if visible {
            self.playtests_ensure_loaded(i);
        }
        self.playtests.time = self.playtests.time.min(self.playtests.longest());
        self.playtests_redraw(cx);
    }

    pub(super) fn playtests_set_heat(&mut self, heat: Option<HeatKind>, cx: &mut Context<Self>) {
        self.playtests.heat = heat;
        self.playtests_redraw(cx);
    }

    pub(super) fn playtests_set_rate(&mut self, rate: f32, cx: &mut Context<Self>) {
        self.playtests.rate_hz = rate;
        self.save_settings();
        cx.notify();
    }

    pub(super) fn playtests_start_rename(
        &mut self,
        path: PathBuf,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        input.update(cx, |state, cx| state.focus(window, cx));
        let subscription = cx.subscribe(&input, |shell, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let name = input.read(cx).value().trim().to_owned();
                shell.playtests_commit_rename(name, cx);
            }
        });
        self.playtests.renaming = Some((path, input));
        self.playtests.rename_subscription = Some(subscription);
        cx.notify();
    }

    fn playtests_commit_rename(&mut self, name: String, cx: &mut Context<Self>) {
        let Some((path, _)) = self.playtests.renaming.take() else {
            return;
        };
        self.playtests.rename_subscription = None;
        if !name.is_empty() {
            if let Err(err) = store::rename(&path, &name) {
                self.output.push(
                    super::super::play::session::PLAY_SOURCE,
                    crate::command_bar::Feedback::Warning(format!(
                        "Could not rename the recording: {err}"
                    )),
                );
            }
        }
        self.playtests_refresh(cx);
    }

    pub(super) fn playtests_cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.playtests.renaming = None;
        self.playtests.rename_subscription = None;
        cx.notify();
    }

    pub(super) fn playtests_delete(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Err(err) = store::delete(&path) {
            self.output.push(
                super::super::play::session::PLAY_SOURCE,
                crate::command_bar::Feedback::Warning(format!(
                    "Could not delete the recording: {err}"
                )),
            );
        }
        self.playtests_refresh(cx);
    }

    /// `RBX_STUDIO_PLAYTESTS`, once: see the module doc.
    pub(super) fn playtests_apply_env(&mut self, cx: &mut Context<Self>) {
        if self.playtests.env_applied {
            return;
        }
        self.playtests.env_applied = true;
        let Ok(value) = std::env::var(super::DEBUG_VARIABLE) else {
            return;
        };
        let heat = match value.trim() {
            "presence" => Some(HeatKind::Presence),
            "deaths" => Some(HeatKind::Deaths),
            "stuck" => Some(HeatKind::Stuck),
            "on" => None,
            _ => return,
        };
        for i in 0..self.playtests.entries.len() {
            self.playtests.entries[i].visible = true;
            self.playtests_ensure_loaded(i);
        }
        self.playtests.heat = heat;
        self.playtests.time = self.playtests.longest() * 0.6;
        self.playtests_redraw(cx);
    }
}
