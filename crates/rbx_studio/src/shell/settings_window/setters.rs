//! The setters Settings needed that no menu had yet: each writes the value,
//! saves, and redraws, so every other place showing it follows.

use gpui_kit::*;

use crate::settings::argon::{Level, Setting, Value};
use crate::transform::{Action, SnapKind};

use super::super::Shell;

impl Shell {
    pub(in crate::shell) fn set_output_timestamps(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.output_show_timestamps = shown;
        self.save_settings();
        cx.notify();
    }

    pub(in crate::shell) fn set_output_collapsed(
        &mut self,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        self.output_collapsed = collapsed;
        self.save_settings();
        cx.notify();
    }

    pub(in crate::shell) fn set_snap_increment(
        &mut self,
        kind: SnapKind,
        increment: f32,
        cx: &mut Context<Self>,
    ) {
        self.transform_action(Action::SetIncrement(kind, increment), cx);
    }

    pub(in crate::shell) fn snap_increment(&self, kind: SnapKind) -> f32 {
        match kind {
            SnapKind::Translate => self.transform.translate.increment,
            SnapKind::Rotate => self.transform.rotate.increment,
        }
    }

    /// Writes `setting` at `level`, which need not be the level the Argon
    /// dock is editing.
    pub(in crate::shell) fn argon_set_at(
        &mut self,
        level: Level,
        setting: Setting,
        value: Value,
        cx: &mut Context<Self>,
    ) {
        let keys = self.argon_level_keys();
        if self.argon_settings.set(setting, value, level, &keys) {
            self.save_settings();
        }
        cx.notify();
    }

    /// Drops `level`'s own override of `setting`.
    pub(in crate::shell) fn argon_clear_at(
        &mut self,
        level: Level,
        setting: Setting,
        cx: &mut Context<Self>,
    ) {
        let keys = self.argon_level_keys();
        if self.argon_settings.clear(setting, level, &keys) {
            self.save_settings();
        }
        cx.notify();
    }

    /// Empties `level` (the dock's Restore defaults, at any level).
    pub(in crate::shell) fn argon_restore_at(&mut self, level: Level, cx: &mut Context<Self>) {
        let keys = self.argon_level_keys();
        if self.argon_settings.restore_defaults(level, &keys) {
            self.save_settings();
        }
        cx.notify();
    }
}
