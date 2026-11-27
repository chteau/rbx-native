//! The menu bar's Test, View and Window items that have no single `Shell`
//! call behind them already: each does what its ribbon tile does.

use gpui_kit::Context;

use super::layout::Panel;
use super::play::{session::StopReason, Phase};
use super::Shell;

impl Shell {
    /// Test › Play: the Test tab's Play, when that tile is live.
    pub(crate) fn menu_play(&self, cx: &mut Context<Self>) {
        if self.play.phase == Phase::Idle && !self.local_run_active() {
            self.with_window(cx, |shell, window, cx| shell.play_start(window, cx));
        }
    }

    /// Test › Stop: whatever the Test tab's Stop tile would end, a local run
    /// (or leaving someone else's), else a real-client test.
    pub(crate) fn menu_stop(&mut self, cx: &mut Context<Self>) {
        if self.local_run_active()
            || self.local_run_role().is_some()
            || self.local_run_remote_viewer()
        {
            self.local_run_stop(cx);
        } else if self.play.phase != Phase::Idle {
            self.play_stop(StopReason::Stopped, cx);
        }
    }

    pub(crate) fn menu_controller_emulator(&mut self, cx: &mut Context<Self>) {
        self.open_controller_emulator(cx);
    }

    /// View › Zoom In / Zoom Out: the UI scale, as Ctrl+= and Ctrl+- step it.
    pub(crate) fn menu_zoom(&mut self, scale: crate::scale::Scale, cx: &mut Context<Self>) {
        self.set_font_scale(scale.apply(crate::tokens::font_scale()), cx);
    }

    pub(crate) fn menu_toggle_show_ui(&mut self, cx: &mut Context<Self>) {
        self.set_screen_gui_shown(!self.show_screen_gui, cx);
    }

    pub(crate) fn menu_toggle_fullbright(&mut self, cx: &mut Context<Self>) {
        self.set_fullbright(!self.fullbright, cx);
    }

    /// Window › Terrain Editor: the Home tab's Terrain, opened on its last
    /// tool or shut.
    pub(crate) fn menu_toggle_terrain_editor(&mut self, cx: &mut Context<Self>) {
        let showing = self.is_panel_showing(Panel::TerrainEditor);
        self.toggle_terrain_editor(!showing, cx);
    }
}
