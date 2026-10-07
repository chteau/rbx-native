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

    pub(in crate::shell) fn set_optimize_graph_on_open(
        &mut self,
        on: bool,
        cx: &mut Context<Self>,
    ) {
        self.optimize_graph_on_open = on;
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

    /// The accent in force, the user's or else the theme's.
    pub(in crate::shell) fn accent(&self) -> gpui_kit::Rgba {
        crate::tokens::check_on()
    }

    /// Makes `accent` the user's own (`None` hands it back to the theme),
    /// applies it everywhere at once and remembers it.
    pub(in crate::shell) fn set_accent(
        &mut self,
        accent: Option<gpui_kit::Rgba>,
        cx: &mut Context<Self>,
    ) {
        self.appearance.accent = accent.map(crate::accent::hex);
        self.save_colors(cx);
    }

    /// One transform tool's colour, by its key (`"move"`); `None` gives it
    /// back to the theme.
    pub(in crate::shell) fn set_tool_color(
        &mut self,
        tool: &str,
        color: Option<gpui_kit::Rgba>,
        cx: &mut Context<Self>,
    ) {
        match color {
            Some(color) => {
                self.appearance
                    .tools
                    .insert(tool.to_owned(), crate::accent::hex(color));
            }
            None => {
                self.appearance.tools.remove(tool);
            }
        }
        self.save_colors(cx);
    }

    pub(in crate::shell) fn reset_tool_colors(&mut self, cx: &mut Context<Self>) {
        self.appearance.tools.clear();
        self.save_colors(cx);
    }

    fn save_colors(&mut self, cx: &mut Context<Self>) {
        if let Err(err) = self.appearance.save_colors() {
            self.output
                .push_warning(&format!("could not remember the colours: {err}"));
        }
        crate::theme::apply(&self.theme, &self.appearance.overrides(), cx);
        cx.notify();
    }

    /// Switches to the installed theme `id` (`theme::DEFAULT_ID` for the
    /// built-in one). An installed theme that brings an accent of its own
    /// seeds the user's accent with it; after that the user's wins, as
    /// always. A built-in one doesn't: Light's darker accent would stay
    /// behind, unreadable, after switching back to a dark theme. A preset
    /// the user picked follows the switch as the new theme's variant of it.
    pub(in crate::shell) fn pick_theme(&mut self, id: &str, cx: &mut Context<Self>) {
        let pack = match crate::theme::ThemePack::load(id) {
            Ok(pack) => pack,
            Err(err) => {
                self.output
                    .push_warning(&format!("theme {id:?} was not applied: {err}"));
                cx.notify();
                return;
            }
        };
        let builtin = crate::theme::ThemePack::builtin();
        let own = pack.palette.color("check_on");
        if own != builtin.palette.color("check_on") && !crate::theme::is_reserved(id) {
            self.appearance.accent = Some(crate::accent::hex(own));
            let _ = self.appearance.save_colors();
        } else if let Some(i) = self.appearance.accent.as_deref().and_then(|text| {
            let current = crate::accent::hex(crate::accent::parse_hex(text)?);
            let from = crate::accent::presets(&self.theme.palette);
            from.iter()
                .position(|(_, color)| crate::accent::hex(*color) == current)
        }) {
            let to = crate::accent::hex(crate::accent::presets(&pack.palette)[i].1);
            self.appearance.accent = (to != crate::accent::hex(own)).then_some(to);
            let _ = self.appearance.save_colors();
        }
        self.appearance.theme = (id != crate::theme::DEFAULT_ID).then(|| id.to_owned());
        if let Err(err) = self.appearance.save_theme() {
            self.output
                .push_warning(&format!("could not remember the theme: {err}"));
        }
        self.reload_theme(cx);
    }

    /// Installs the theme `link` names off the UI thread, then applies it.
    /// The task is the shell's, not Settings', so closing Settings
    /// mid-download still applies the theme; a failure shows inline while
    /// Settings is open, and in Output once it has been closed. A link
    /// that isn't a GitHub repository fails at once.
    pub(in crate::shell) fn install_theme(&mut self, link: String, cx: &mut Context<Self>) {
        use super::ThemeInstall;
        if matches!(self.theme_install, ThemeInstall::Running) {
            return;
        }
        if let Err(err) = crate::theme::Source::parse(&link) {
            self.theme_install = ThemeInstall::Failed(err.into());
            cx.notify();
            return;
        }
        self.theme_install = ThemeInstall::Running;
        cx.notify();
        cx.spawn(async move |shell, cx| {
            let from = link.clone();
            let installed = cx
                .background_spawn(async move { crate::theme::install(&from) })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                let settings_open = shell
                    .settings_window
                    .is_some_and(|window| window.is_active(cx).is_some());
                shell.theme_install = match installed {
                    Ok((id, _)) => {
                        shell.pick_theme(&id, cx);
                        ThemeInstall::Idle
                    }
                    Err(err) if settings_open => ThemeInstall::Failed(err.into()),
                    Err(err) => {
                        shell
                            .output
                            .push_warning(&format!("theme {link:?} was not installed: {err}"));
                        ThemeInstall::Idle
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Deletes the installed theme `id`, switching to Default first when
    /// it is the one in use.
    pub(in crate::shell) fn uninstall_theme(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.appearance.theme.as_deref() == Some(id) {
            self.pick_theme(crate::theme::DEFAULT_ID, cx);
        }
        let removed = crate::theme::themes_dir()
            .ok_or_else(|| "there is no config directory".to_owned())
            .and_then(|dir| crate::theme::uninstall(&dir, id));
        if let Err(err) = removed {
            self.output
                .push_warning(&format!("theme {id:?} was not uninstalled: {err}"));
        }
        cx.notify();
    }
}
