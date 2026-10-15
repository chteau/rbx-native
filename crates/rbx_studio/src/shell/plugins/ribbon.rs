//! The Plugins ribbon tab: one group per plugin toolbar (and per WASM
//! plugin's commands), then Manage / Folder / Reload.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui_kit::assets::IconName;
use gpui_kit::component::{v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_lua::plugin::Input;

use crate::shell::Shell;
use crate::tokens;

/// Asset ids a download is already running for, so a redraw does not start
/// the same request again.
static REQUESTED: Mutex<Option<HashSet<u64>>> = Mutex::new(None);

impl Shell {
    /// The tab's groups, as `ribbon` lays them out. A button is a tile with
    /// its icon (resolved by `icon_path`, a generic plug icon otherwise),
    /// its text, its tooltip, the selected look while `active`, disabled
    /// while not `enabled`; clicking it sends `Input::ButtonClicked`.
    pub(in crate::shell) fn plugin_ribbon_groups(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        let mut groups = Vec::new();

        for toolbar in &self.plugins.ui.toolbars {
            let mut tiles = Vec::new();
            for button in &toolbar.buttons {
                let id = button.id;
                let icon = icon_path(&button.icon);
                let mut tile = plugin_tile(
                    ("plugin-button", id as usize),
                    SharedString::from(button.text.clone()),
                    icon,
                    button.enabled,
                );
                if button.active && button.enabled {
                    tile = crate::shell::ribbon::selected(tile, crate::launcher::ui::accent());
                }
                if !button.tooltip.is_empty() {
                    let tip = format!("{} — {}", button.text, button.tooltip);
                    tile = tile.tooltip(move |window, cx| {
                        crate::shell::tooltip::text(tip.clone(), window, cx)
                    });
                }
                if button.enabled {
                    tile = self.ribbon_nav.claim(tile, cx).on_click(cx.listener(
                        move |shell, _, _, cx| shell.plugin_input(Input::ButtonClicked(id), cx),
                    ));
                }
                tiles.push(tile.into_any_element());
            }
            if !tiles.is_empty() {
                groups.push(tiles);
            }
        }

        for (index, entry) in self.plugins.wasm.iter().enumerate() {
            let mut tiles = Vec::new();
            for (command, text, tooltip) in &entry.commands {
                let command_id = command.clone();
                let mut tile = plugin_tile(
                    ElementId::from(SharedString::from(format!("wasm-{index}-{command}"))),
                    SharedString::from(text.clone()),
                    None,
                    true,
                );
                if !tooltip.is_empty() {
                    let tip = format!("{text} — {tooltip}");
                    tile = tile.tooltip(move |window, cx| {
                        crate::shell::tooltip::text(tip.clone(), window, cx)
                    });
                }
                tile = self.ribbon_nav.claim(tile, cx).on_click(cx.listener(
                    move |shell, _, _, cx| shell.wasm_command(index, command_id.clone(), cx),
                ));
                tiles.push(tile.into_any_element());
            }
            if !tiles.is_empty() {
                groups.push(tiles);
            }
        }

        let manage = self
            .ribbon_nav
            .claim(
                plugin_tile("plugins-manage", "Manage".into(), None, true).tooltip(|window, cx| {
                    crate::shell::tooltip::text("Manage Plugins", window, cx)
                }),
                cx,
            )
            .on_click(cx.listener(|shell, _, _, cx| shell.open_plugin_manager(cx)));
        let folder = self
            .ribbon_nav
            .claim(
                plugin_tile("plugins-folder", "Folder".into(), None, true).tooltip(|window, cx| {
                    crate::shell::tooltip::text("Open the plugins folder", window, cx)
                }),
                cx,
            )
            .on_click(cx.listener(|_, _, _, cx| {
                if let Some(dir) = super::discover::own_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    cx.open_url(&format!("file://{}", dir.display()));
                }
            }));
        let reload = self
            .ribbon_nav
            .claim(
                plugin_tile("plugins-reload", "Reload".into(), None, true).tooltip(|window, cx| {
                    crate::shell::tooltip::text("Reload all plugins", window, cx)
                }),
                cx,
            )
            .on_click(cx.listener(|shell, _, _, cx| shell.plugins_reload(cx)));
        groups.push(vec![
            manage.into_any_element(),
            folder.into_any_element(),
            reload.into_any_element(),
        ]);

        groups
    }
}

/// A ribbon tile with a dynamic label, styled like the frame's own tiles.
/// A `None` icon draws the generic plugin glyph.
fn plugin_tile(
    id: impl Into<ElementId>,
    label: SharedString,
    icon: Option<PathBuf>,
    enabled: bool,
) -> Stateful<Div> {
    let size = px(20.);
    let glyph = match icon {
        Some(path) => img(Arc::<Path>::from(path.as_path()))
            .size(size)
            .into_any_element(),
        None => Icon::new(IconName::Settings).size(size).into_any_element(),
    };
    v_flex()
        .id(id.into())
        .relative()
        .flex_none()
        .w(tokens::tile_width())
        .h_full()
        .gap(px(6.))
        .px(px(1.))
        .py(px(5.))
        .items_center()
        .justify_center()
        .rounded(tokens::radius())
        .bg(tokens::tile())
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .map(|this| {
            if enabled {
                this.focus_visible(|this| this.shadow(tokens::focus_ring(tokens::chrome())))
                    .cursor_pointer()
                    .text_color(tokens::text_label())
                    .hover(|this| {
                        tokens::hover_fx(this)
                            .bg(tokens::hover())
                            .text_color(tokens::text_full())
                    })
                    .active(|this| this.bg(tokens::ribbon_tab_active()))
            } else {
                this.cursor_not_allowed()
                    .text_color(tokens::text_disabled())
            }
        })
        .child(
            div()
                .flex_none()
                .size(size)
                .flex()
                .items_center()
                .justify_center()
                .child(glyph),
        )
        .child(div().w_full().text_center().truncate().child(label))
}

/// A local image for a plugin icon: an existing file path as is,
/// `rbxasset://textures/...` under Studio's content folder when found,
/// `rbxassetid://<id>` from the asset cache once downloaded (requesting it
/// otherwise); `None` while unavailable.
pub(crate) fn icon_path(icon: &str) -> Option<std::path::PathBuf> {
    let icon = icon.trim();
    if icon.is_empty() {
        return None;
    }
    let assets = rbx_assets::cache_root()?.join("assets");
    if let Some(relative) = icon.strip_prefix("rbxasset://") {
        let path = assets.join("native").join(relative);
        return path.is_file().then_some(path);
    }
    if let Some(id) = icon.strip_prefix("rbxassetid://") {
        let id: u64 = id.trim().parse().ok()?;
        let path = assets.join("ids").join(id.to_string());
        if path.is_file() {
            return Some(path);
        }
        request_download(id);
        return None;
    }
    let path = PathBuf::from(icon);
    path.is_file().then_some(path)
}

fn request_download(id: u64) {
    {
        let mut requested = REQUESTED.lock().unwrap_or_else(|e| e.into_inner());
        if !requested.get_or_insert_with(HashSet::new).insert(id) {
            return;
        }
    }
    std::thread::spawn(move || {
        let client = rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config());
        if let (Ok(content), Ok(cache)) = (client.asset(id), rbx_assets::AssetCache::new(None)) {
            let _ = cache.put_id(id, &content.bytes);
        }
    });
}
