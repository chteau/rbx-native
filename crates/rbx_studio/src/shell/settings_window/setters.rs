//! The setters Settings needed that no menu had yet: each writes the value,
//! saves, and redraws, so every other place showing it follows.

use gpui_kit::*;

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
}
