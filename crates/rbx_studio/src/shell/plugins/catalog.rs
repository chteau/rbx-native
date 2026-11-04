//! Popular plugins: the Manage Plugins dialog's second tab. It lists the
//! verified free plugins (`list`), refreshed with the store's live name,
//! votes and update time, and searches the Creator Store for more. One
//! click downloads a plugin from Roblox with the user's credentials,
//! writes it into the plugins folder (`installs`) and starts it in the
//! running host; Update and Uninstall swap or stop it the same way.
//!
//! The store routes are anonymous; only the download needs the Open Cloud
//! key, as `rbx_cloud::Client::asset` falls back to it when anonymous
//! delivery refuses (it does for plugins).

mod fetch;
mod installs;
mod list;
mod rows;
pub(super) mod view;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_cloud::StoreItem;
use rbx_lua::plugin::{Input, Origin};

use super::discover;
use crate::command_bar::Feedback;
use crate::shell::Shell;
use fetch::{client, download, fetch_icons};
/// The store routes and thumbnails, shared with the Toolbox.
pub(in crate::shell) use fetch::{client as store_client, fetch_icons as store_icons};
use installs::{Installs, Record};
pub(super) use rows::{Row, State};

/// Search hits asked for; paid ones are dropped after.
const SEARCH_LIMIT: u32 = 30;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Installed,
    Popular,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) enum Load {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed(String),
}

/// A live search: what was asked, the free hits in store order.
#[derive(Default)]
struct Search {
    text: String,
    ids: Vec<u64>,
    load: Load,
    generation: u64,
}

#[derive(Default)]
pub(crate) struct Catalog {
    pub(super) tab: Tab,
    /// The store's listing of every plugin shown so far.
    items: HashMap<u64, StoreItem>,
    /// The verified list's refresh.
    listing: Load,
    /// `None` once an icon failed, so the glyph stays.
    icons: HashMap<u64, Option<Arc<RenderImage>>>,
    icons_asked: HashSet<u64>,
    /// Downloads in flight.
    busy: HashSet<u64>,
    failures: HashMap<u64, String>,
    installs: Installs,
    query: Option<Entity<InputState>>,
    _query_entered: Option<Subscription>,
    search: Search,
}

fn records_path() -> Option<PathBuf> {
    crate::settings::default_config_dir().map(|dir| dir.join("plugin-catalog.json"))
}

impl Shell {
    /// Opens Manage Plugins on the Popular tab.
    pub(super) fn open_plugin_catalog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_plugin_manager(window, cx);
        self.catalog_show(Tab::Popular, window, cx);
    }

    /// Switches tab; Popular loads the records, the search field and the
    /// verified list's store data the first time it shows.
    pub(super) fn catalog_show(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.plugins.catalog.tab = tab;
        if tab == Tab::Popular {
            if let Some(path) = records_path() {
                self.plugins.catalog.installs = Installs::load_from(&path);
            }
            if self.plugins.catalog.query.is_none() {
                let query = cx.new(|cx| {
                    InputState::new(window, cx).placeholder("Search the Creator Store for plugins")
                });
                let entered = cx.subscribe(&query, |shell, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        shell.catalog_search(cx);
                    }
                });
                self.plugins.catalog.query = Some(query);
                self.plugins.catalog._query_entered = Some(entered);
            }
            if matches!(self.plugins.catalog.listing, Load::Idle | Load::Failed(_)) {
                self.catalog_refresh(cx);
            }
        } else if let Some(query) = &self.plugins.filter.query {
            query.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    /// Loads the install records and the store listing of the installed
    /// plugins not listed yet, so the Installed tab can name and search by
    /// their creators.
    pub(super) fn catalog_load_installed(&mut self, cx: &mut Context<Self>) {
        let catalog = &mut self.plugins.catalog;
        if let Some(path) = records_path() {
            catalog.installs = Installs::load_from(&path);
        }
        let ids: Vec<u64> = catalog
            .installs
            .installed
            .keys()
            .filter(|id| !catalog.items.contains_key(id))
            .copied()
            .collect();
        if ids.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_spawn(async move { client().store_items(&ids) })
                .await;
            if let Ok(items) = reply {
                let _ = this.update(cx, |shell, cx| {
                    let items = items.into_iter().map(|item| (item.id, item));
                    shell.plugins.catalog.items.extend(items);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The store creator of each plugin the catalog installed, by file name.
    pub(super) fn catalog_creators(&self) -> HashMap<&str, &str> {
        let catalog = &self.plugins.catalog;
        catalog
            .installs
            .installed
            .iter()
            .filter_map(|(id, record)| {
                let item = catalog.items.get(id)?;
                Some((record.file.as_str(), item.creator_name.as_str()))
            })
            .collect()
    }

    fn catalog_refresh(&mut self, cx: &mut Context<Self>) {
        self.plugins.catalog.listing = Load::Loading;
        let ids: Vec<u64> = list::VERIFIED.iter().map(|entry| entry.id).collect();
        self.catalog_icons(&ids, cx);
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_spawn(async move { client().store_items(&ids) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                let catalog = &mut shell.plugins.catalog;
                match reply {
                    Ok(items) => {
                        catalog
                            .items
                            .extend(items.into_iter().map(|item| (item.id, item)));
                        catalog.listing = Load::Ready;
                    }
                    Err(err) => catalog.listing = Load::Failed(err.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetches the icons of `ids` not asked for yet.
    fn catalog_icons(&mut self, ids: &[u64], cx: &mut Context<Self>) {
        let catalog = &mut self.plugins.catalog;
        let wanted: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| catalog.icons_asked.insert(*id))
            .collect();
        if wanted.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let icons = cx
                .background_spawn(async move { fetch_icons(&wanted) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.plugins.catalog.icons.extend(icons);
                cx.notify();
            });
        })
        .detach();
    }

    /// Runs the field's query; an empty one goes back to the verified list.
    fn catalog_search(&mut self, cx: &mut Context<Self>) {
        let Some(query) = &self.plugins.catalog.query else {
            return;
        };
        let text = query.read(cx).value().trim().to_owned();
        let search = &mut self.plugins.catalog.search;
        search.generation += 1;
        search.text = text.clone();
        search.ids.clear();
        if text.is_empty() {
            search.load = Load::Idle;
            cx.notify();
            return;
        }
        search.load = Load::Loading;
        let generation = search.generation;
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_spawn(async move {
                    let client = client();
                    let ids = client.search_plugins(&text, SEARCH_LIMIT)?;
                    let items = client.store_items(&ids)?;
                    Ok::<_, rbx_cloud::CloudError>((ids, items))
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                if shell.plugins.catalog.search.generation != generation {
                    return;
                }
                match reply {
                    Ok((ids, items)) => {
                        let free: HashSet<u64> = items
                            .iter()
                            .filter(|item| item.free)
                            .map(|item| item.id)
                            .collect();
                        let ids: Vec<u64> =
                            ids.into_iter().filter(|id| free.contains(id)).collect();
                        let catalog = &mut shell.plugins.catalog;
                        catalog
                            .items
                            .extend(items.into_iter().map(|item| (item.id, item)));
                        catalog.search.ids = ids.clone();
                        catalog.search.load = Load::Ready;
                        shell.catalog_icons(&ids, cx);
                    }
                    Err(err) => shell.plugins.catalog.search.load = Load::Failed(err.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Installs `id`, or replaces an installed copy with the store's
    /// current one. Paid plugins are refused before anything downloads.
    pub(super) fn catalog_install(&mut self, id: u64, cx: &mut Context<Self>) {
        let catalog = &mut self.plugins.catalog;
        if catalog.busy.contains(&id) {
            return;
        }
        if catalog.items.get(&id).is_some_and(|item| !item.free) {
            catalog
                .failures
                .insert(id, "Only free plugins can be installed.".into());
            cx.notify();
            return;
        }
        let Some(dir) = discover::own_dir() else {
            return;
        };
        let name = list::verified(id)
            .map(|entry| entry.name.to_owned())
            .or_else(|| catalog.items.get(&id).map(|item| item.name.clone()))
            .unwrap_or_else(|| format!("Plugin {id}"));
        let path = installs::target(&dir, &catalog.installs, id, &name);
        let updated = catalog.items.get(&id).map(|item| item.updated.clone());
        catalog.busy.insert(id);
        catalog.failures.remove(&id);
        cx.notify();
        let written = path.clone();
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_spawn(async move {
                    let bytes = download(id)?;
                    installs::write(&written, &bytes)
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.plugins.catalog.busy.remove(&id);
                match reply {
                    Ok(()) => {
                        let file = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let record = Record {
                            file,
                            updated: updated.unwrap_or_default(),
                        };
                        shell.plugins.catalog.installs.installed.insert(id, record);
                        shell.catalog_save();
                        shell.plugins_swap_file(&path, true, cx);
                    }
                    Err(message) => {
                        shell.plugins.catalog.failures.insert(id, message);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Stops `id`, deletes its file and forgets it.
    pub(super) fn catalog_uninstall(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(dir) = discover::own_dir() else {
            return;
        };
        let Some(record) = self.plugins.catalog.installs.installed.remove(&id) else {
            return;
        };
        let path = dir.join(&record.file);
        self.plugins_swap_file(&path, false, cx);
        if let Err(err) = std::fs::remove_file(&path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                self.plugins.catalog.failures.insert(id, err.to_string());
                self.plugins.catalog.installs.installed.insert(id, record);
                cx.notify();
                return;
            }
        }
        self.catalog_save();
        self.plugins_rescan();
        cx.notify();
    }

    fn catalog_save(&mut self) {
        if let Some(path) = records_path() {
            if let Err(message) = self.plugins.catalog.installs.save_to(&path) {
                self.output
                    .push_plugin("Plugins", Feedback::Warning(message));
            }
        }
    }

    /// Unloads whatever the host runs from `path`, then, when `start`,
    /// loads the file again and starts it; the other plugins keep running.
    fn plugins_swap_file(&mut self, path: &Path, start: bool, cx: &mut Context<Self>) {
        let running: Vec<_> = self
            .plugins
            .host
            .as_ref()
            .map(|host| {
                host.plugins()
                    .into_iter()
                    .filter(|info| match &info.origin {
                        Origin::Roblox { path: from } | Origin::Native { path: from, .. } => {
                            from == path
                        }
                    })
                    .map(|info| info.id)
                    .collect()
            })
            .unwrap_or_default();
        for id in running {
            self.plugin_input(Input::Unload(id), cx);
        }
        if start {
            match rbx_lua::plugin::read_source(path) {
                Ok(source) => {
                    if let Some(host) = self.plugins.host.as_mut() {
                        host.add(source);
                        self.plugin_input(Input::Start, cx);
                        self.plugins_arm_poll(cx);
                    }
                }
                Err(message) => {
                    let name = path.file_stem().unwrap_or_default().to_string_lossy();
                    self.output.push_plugin(&name, Feedback::Warning(message));
                }
            }
        }
        self.plugins_rescan();
    }
}
