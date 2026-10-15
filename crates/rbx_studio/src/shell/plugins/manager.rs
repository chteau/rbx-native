//! The Manage Plugins dialog: every plugin found, where it came from,
//! whether it runs, its last error; a switch per plugin (persisted), the
//! "Load Roblox Studio's plugins" switch, Open Folder and Reload.

use std::cell::RefCell;
use std::path::Path;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_lua::plugin::Origin;

use super::discover::{self, Found, Kind};
use crate::launcher::ui::{self, Weight};
use crate::shell::Shell;
use crate::tokens;

thread_local! {
    /// Held by the open dialog so Escape reaches it (the UI is single
    /// threaded).
    static FOCUS: RefCell<Option<FocusHandle>> = const { RefCell::new(None) };
}

/// What the user chose in the dialog, kept in `<config dir>/plugins.json`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Prefs {
    /// Paths of plugins switched off.
    pub(crate) disabled: Vec<String>,
    /// Also load Roblox Studio's own Plugins folder (on by default).
    pub(crate) load_studio: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            load_studio: true,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct PrefsFile {
    disabled: Vec<String>,
    load_studio: bool,
}

impl Default for PrefsFile {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            load_studio: true,
        }
    }
}

fn prefs_path() -> Option<std::path::PathBuf> {
    crate::settings::default_config_dir().map(|dir| dir.join("plugins.json"))
}

fn load_from(path: &Path) -> Prefs {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<PrefsFile>(&text).ok())
        .map(|file| Prefs {
            disabled: file.disabled,
            load_studio: file.load_studio,
        })
        .unwrap_or_default()
}

fn save_to(path: &Path, prefs: &Prefs) {
    let file = PrefsFile {
        disabled: prefs.disabled.clone(),
        load_studio: prefs.load_studio,
    };
    if let Ok(text) = serde_json::to_string_pretty(&file) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = crate::settings::write_atomic(path, text.as_bytes());
    }
}

/// The saved prefs, or the defaults when there are none (or they do not
/// parse).
pub(crate) fn load_prefs() -> Prefs {
    prefs_path()
        .map(|path| load_from(&path))
        .unwrap_or_default()
}

/// Saves atomically (`settings::write_atomic`); failures are ignored, as
/// a lost preference costs a click.
pub(crate) fn save_prefs(prefs: &Prefs) {
    if let Some(path) = prefs_path() {
        save_to(&path, prefs);
    }
}

fn kind_label(kind: &Kind) -> &'static str {
    match kind {
        Kind::Roblox => "Roblox",
        Kind::NativeLuau => "Native Luau",
        Kind::NativeWasm => "Native WASM",
    }
}

fn switch(id: SharedString, on: bool) -> Stateful<Div> {
    let track = div()
        .id(id)
        .w(px(34.))
        .h(px(18.))
        .rounded_full()
        .cursor_pointer()
        .flex()
        .items_center()
        .px(px(2.))
        .bg(if on { ui::accent() } else { tokens::border() });
    let thumb = div().size(px(14.)).rounded_full().bg(tokens::text());
    if on {
        track.justify_end().child(thumb)
    } else {
        track.justify_start().child(thumb)
    }
}

impl Shell {
    pub(super) fn open_plugin_manager(&mut self, cx: &mut Context<Self>) {
        FOCUS.with(|slot| {
            slot.borrow_mut().get_or_insert_with(|| cx.focus_handle());
        });
        self.plugins.manager_open = true;
        cx.notify();
    }

    /// Running / error / off, for one found plugin.
    fn plugin_status(&self, found: &Found, disabled: bool) -> (String, bool) {
        if disabled {
            return ("Off".into(), false);
        }
        if found.kind == Kind::NativeWasm {
            return match self
                .plugins
                .wasm
                .iter()
                .find(|entry| entry.name == found.name)
            {
                Some(entry) => match &entry.error {
                    Some(error) => (error.clone(), true),
                    None => ("Running".into(), false),
                },
                None => ("Not loaded".into(), false),
            };
        }
        let info = self.plugins.host.as_ref().and_then(|host| {
            host.plugins().into_iter().find(|info| match &info.origin {
                Origin::Roblox { path } | Origin::Native { path, .. } => *path == found.path,
            })
        });
        match info {
            Some(info) => match info.error {
                Some(error) => (error, true),
                None if info.started => ("Running".into(), false),
                None => ("Starting".into(), false),
            },
            None => ("Not loaded".into(), false),
        }
    }

    pub(super) fn plugin_manager_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.plugins.manager_open {
            return None;
        }
        let focus = FOCUS.with(|slot| slot.borrow().clone())?;
        if !focus.is_focused(window) {
            focus.focus(window, cx);
        }
        let load_studio = load_prefs().load_studio;

        let mut rows = v_flex().gap(px(6.)).max_h(px(320.)).overflow_hidden();
        if self.plugins.found.is_empty() {
            rows = rows.child(div().text_color(tokens::text2()).child("No plugins found."));
        }
        for (index, found) in self.plugins.found.iter().enumerate() {
            let key = found.path.to_string_lossy().to_string();
            let disabled = self.plugins.disabled.contains(&key);
            let (status, is_error) = self.plugin_status(found, disabled);
            let tag = if found.from_studio {
                format!("{} \u{b7} from Studio", kind_label(&found.kind))
            } else {
                kind_label(&found.kind).to_string()
            };
            let status_color = if is_error {
                tokens::text_error()
            } else {
                tokens::text2()
            };
            rows = rows.child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .child(
                        switch(format!("plugin-switch-{index}").into(), !disabled).on_click(
                            cx.listener(move |shell, _, _, cx| {
                                let mut prefs = load_prefs();
                                if shell.plugins.disabled.contains(&key) {
                                    shell.plugins.disabled.remove(&key);
                                } else {
                                    shell.plugins.disabled.insert(key.clone());
                                }
                                let mut names: Vec<String> =
                                    shell.plugins.disabled.iter().cloned().collect();
                                names.sort();
                                prefs.disabled = names;
                                save_prefs(&prefs);
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .child(div().text_color(tokens::text()).child(found.name.clone()))
                            .child(
                                div()
                                    .text_color(tokens::text2())
                                    .text_size(px(11.))
                                    .child(tag),
                            ),
                    )
                    .child(
                        div()
                            .text_color(status_color)
                            .text_size(px(11.))
                            .max_w(px(220.))
                            .child(status),
                    ),
            );
        }
        let studio_row = h_flex()
            .gap(px(10.))
            .items_center()
            .child(
                switch("plugin-load-studio".into(), load_studio).on_click(cx.listener(
                    |_, _, _, cx| {
                        let mut prefs = load_prefs();
                        prefs.load_studio = !prefs.load_studio;
                        save_prefs(&prefs);
                        cx.notify();
                    },
                )),
            )
            .child(
                div()
                    .text_color(tokens::text())
                    .child("Load Roblox Studio plugins"),
            )
            .child(
                div()
                    .text_color(tokens::text2())
                    .text_size(px(11.))
                    .child("Changes take effect on Reload."),
            );
        let body = v_flex()
            .px(px(20.))
            .pt(px(14.))
            .gap(px(12.))
            .child(rows)
            .child(studio_row)
            .into_any_element();

        Some(
            div()
                .absolute()
                .inset_0()
                .track_focus(&focus)
                .on_key_down(cx.listener(|shell, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        shell.plugins.manager_open = false;
                        cx.notify();
                    }
                }))
                .child(ui::dialog(
                    560.,
                    ui::dialog_glyph("plug-2", ui::accent(), ui::wash()),
                    "Manage Plugins",
                    "Switches apply the next time plugins load.".to_string(),
                    Some(body),
                    vec![
                        ui::button("plugins-open-folder", "Open Folder", Weight::Ghost, false)
                            .on_click(cx.listener(|_, _, _, cx| {
                                if let Some(dir) = discover::own_dir() {
                                    let _ = std::fs::create_dir_all(&dir);
                                    cx.reveal_path(&dir);
                                }
                            }))
                            .into_any_element(),
                        ui::button("plugins-reload", "Reload", Weight::Secondary, false)
                            .on_click(cx.listener(|shell, _, _, cx| shell.plugins_reload(cx)))
                            .into_any_element(),
                        ui::button("plugins-close", "Close", Weight::Primary, false)
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.plugins.manager_open = false;
                                cx.notify();
                            }))
                            .into_any_element(),
                    ],
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{load_from, save_to, Prefs};

    #[test]
    fn prefs_round_trip() {
        let dir = std::env::temp_dir().join(format!("rbx-plugin-prefs-{}", std::process::id()));
        let path = dir.join("plugins.json");
        let prefs = Prefs {
            disabled: vec!["/a/b.lua".into(), "/c".into()],
            load_studio: false,
        };
        save_to(&path, &prefs);
        assert_eq!(load_from(&path), prefs);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let path = std::env::temp_dir().join("rbx-plugin-prefs-missing/plugins.json");
        assert_eq!(load_from(&path), Prefs::default());
        assert!(Prefs::default().load_studio);
    }
}
