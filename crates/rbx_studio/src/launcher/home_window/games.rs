//! Where My Games comes from: the disk cache first, so the page fills at
//! once, then Roblox in the background — the personal listing, a group's
//! games when that group is picked in the owner dropdown, and the icons,
//! downloading only the ones whose URL changed.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::*;
use rbx_cloud::{ApiKey, Client, Experience, Experiences, Owner};

use super::{decode_icon, fixtures, Games, HomeWindow, KeyState};
use crate::home;
use crate::launcher::cache::{self, GamesCache};

/// Icons downloaded at once: a CDN, not the rate-limited games API.
const ICON_DOWNLOADS: usize = 8;

impl HomeWindow {
    /// (Re)reads the key and My Games — at open, and after the key was
    /// replaced or removed in Roblox publishing.
    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        self.recent = home::recent();
        if let Ok(which) = std::env::var(fixtures::GAMES_VARIABLE) {
            self.load_fixture(&which);
            return;
        }
        let Some(key) = ApiKey::from_env_or_config() else {
            self.key = KeyState::Missing;
            self.games = Games::Loaded(Experiences::default());
            self.set_groups(Vec::new());
            cx.notify();
            return;
        };
        self.key = KeyState::Checking;
        // The last account's listing, straight from disk: the page fills
        // before any request, and the key check below confirms it is the
        // same account (or drops it).
        if self.user_id.is_none() {
            if let Some((user_id, cached)) =
                GamesCache::last_user().and_then(|id| Some((id, GamesCache::load(id)?)))
            {
                self.user_id = Some(user_id);
                self.games = Games::Loaded(cached.listing.clone());
                self.set_groups(cached.listing.groups.clone());
                self.cache = cached;
                self.refreshing = true;
                self.load_cached_icons(cx);
            }
        }
        cx.notify();
        let client = Client::new(Some(key));
        cx.spawn(async move |this, cx| {
            let checked = cx
                .background_spawn({
                    let client = client.clone();
                    async move {
                        let info = client.introspect().ok()?;
                        let owner = client
                            .user_display_name(info.authorized_user_id)
                            .unwrap_or_else(|_| format!("User {}", info.authorized_user_id));
                        Some((
                            info.authorized_user_id,
                            owner,
                            rbx_cloud::check_scopes(&info),
                        ))
                    }
                })
                .await;
            let Ok(user_id) = this.update(cx, |this, cx| {
                let Some((user_id, owner, report)) = checked else {
                    this.key = KeyState::Unchecked;
                    this.refreshing = false;
                    cx.notify();
                    return None;
                };
                this.key = KeyState::Ready { owner, report };
                // Another account than the one shown: its own cache, or none.
                if this.user_id != Some(user_id) {
                    this.icons.clear();
                    this.owner = None;
                    this.user_id = Some(user_id);
                    match GamesCache::load(user_id) {
                        Some(cached) => {
                            this.games = Games::Loaded(cached.listing.clone());
                            this.set_groups(cached.listing.groups.clone());
                            this.cache = cached;
                            this.load_cached_icons(cx);
                        }
                        None => {
                            this.cache = GamesCache::default();
                            this.games = Games::Loading;
                        }
                    }
                }
                this.refreshing = true;
                cx.notify();
                Some(user_id)
            }) else {
                return;
            };
            let Some(user_id) = user_id else {
                return;
            };
            let listing = cx
                .background_spawn({
                    let client = client.clone();
                    async move { client.list_experiences() }
                })
                .await;
            let ids = this.update(cx, |this, cx| {
                this.refreshing = false;
                let ids = match listing {
                    Ok(listing) => {
                        let ids = listing.experiences.iter().map(|e| e.universe_id).collect();
                        this.set_groups(listing.groups.clone());
                        this.cache.listing = listing.clone();
                        this.cache.save(user_id);
                        this.games = Games::Loaded(listing);
                        ids
                    }
                    // A failed refresh keeps what the cache showed.
                    Err(err) => {
                        if !matches!(this.games, Games::Loaded(_)) {
                            this.games = Games::Failed(err.to_string());
                        }
                        Vec::new()
                    }
                };
                cx.notify();
                ids
            });
            if let Ok(ids) = ids {
                let _ = this.update(cx, |this, cx| this.refresh_icons(ids, cx));
            }
            // A group picked before the refresh started gets fetched too.
            let _ = this.update(cx, |this, cx| {
                if let Some(group) = this.owner {
                    this.fetch_group(group, cx);
                }
            });
        })
        .detach();
    }

    fn load_fixture(&mut self, which: &str) {
        let key = std::env::var(super::super::key_check::FIXTURE_VARIABLE)
            .unwrap_or_else(|_| "ready".to_string());
        if let super::super::key_check::Status::Done(checked) = fixtures::key_status(&key) {
            self.key = KeyState::Ready {
                owner: checked.owner,
                report: checked.report,
            };
        }
        if which == "nokey" {
            self.key = KeyState::Missing;
        }
        self.games = match fixtures::games(which) {
            Some(games) => {
                self.set_groups(games.groups.clone());
                Games::Loaded(games)
            }
            None => Games::Loading,
        };
    }

    /// Decodes the cached icons off the UI thread; cards show their
    /// placeholder until then.
    fn load_cached_icons(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self
            .cache
            .listing
            .experiences
            .iter()
            .chain(self.cache.group_games.values().flatten())
            .map(|e| e.universe_id)
            .collect();
        cx.spawn(async move |this, cx| {
            let icons = cx.background_spawn(async move { cached_icons(&ids) }).await;
            let _ = this.update(cx, |this, cx| {
                for (id, image) in icons {
                    this.icons.entry(id).or_insert(image);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Picked in the owner dropdown: `None` is the key's own account.
    pub(super) fn pick_owner(&mut self, owner: Option<u64>, cx: &mut Context<Self>) {
        self.owner = owner;
        if let Some(group) = owner {
            self.fetch_group(group, cx);
        }
        cx.notify();
    }

    /// One group's games, from Roblox; the cached copy shows meanwhile.
    fn fetch_group(&mut self, group: u64, cx: &mut Context<Self>) {
        if std::env::var(fixtures::GAMES_VARIABLE).is_ok() {
            return;
        }
        let (Some(key), Some(user_id)) = (ApiKey::from_env_or_config(), self.user_id) else {
            return;
        };
        self.group_loading = Some(group);
        let client = Client::new(Some(key));
        cx.spawn(async move |this, cx| {
            let games = cx
                .background_spawn(async move { client.group_experiences(group) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.group_loading == Some(group) {
                    this.group_loading = None;
                }
                if let Ok(games) = games {
                    let ids = games.iter().map(|g| g.universe_id).collect();
                    this.cache.group_games.insert(group, games);
                    this.cache.save(user_id);
                    this.refresh_icons(ids, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks for the current icon URLs of `ids` and downloads only those not
    /// already cached from the same URL, a few at a time — each batch shows
    /// and is saved as it lands, so a slow download never holds back the rest
    /// and quitting halfway keeps what arrived.
    fn refresh_icons(&mut self, ids: Vec<u64>, cx: &mut Context<Self>) {
        let (Some(key), Some(user_id)) = (ApiKey::from_env_or_config(), self.user_id) else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let known = self.cache.icon_urls.clone();
        let have: Vec<u64> = self.icons.keys().copied().collect();
        let client = Client::new(Some(key));
        cx.spawn(async move |this, cx| {
            let stale: Vec<(u64, String)> = cx
                .background_spawn({
                    let client = client.clone();
                    async move {
                        client
                            .game_icon_urls(&ids)
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|(id, url)| known.get(id) != Some(url) || !have.contains(id))
                            .collect()
                    }
                })
                .await;
            for batch in stale.chunks(ICON_DOWNLOADS).map(<[_]>::to_vec) {
                let client = client.clone();
                let fetched = cx
                    .background_spawn(async move { download_icons(&client, batch) })
                    .await;
                let alive = this.update(cx, |this, cx| {
                    for (universe_id, url, image) in fetched {
                        this.cache.icon_urls.insert(universe_id, url);
                        this.icons.insert(universe_id, image);
                    }
                    this.cache.save(user_id);
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// The games the page shows for the picked owner: the account's own, or
    /// one group's — its public list plus any of its experiences the key
    /// reached another way (a restricted scope).
    pub(super) fn owner_games(&self) -> Vec<Experience> {
        let Games::Loaded(listing) = &self.games else {
            return Vec::new();
        };
        match self.owner {
            None => listing
                .experiences
                .iter()
                .filter(|e| matches!(e.owner, Owner::User(_)))
                .cloned()
                .collect(),
            Some(group) => {
                let mut games: Vec<Experience> = self
                    .cache
                    .group_games
                    .get(&group)
                    .cloned()
                    .unwrap_or_default();
                for e in &listing.experiences {
                    if e.owner == Owner::Group(group)
                        && !games.iter().any(|g| g.universe_id == e.universe_id)
                    {
                        games.push(e.clone());
                    }
                }
                games
            }
        }
    }
}

/// Every cached icon, decoded — what the page shows before any network.
fn cached_icons(ids: &[u64]) -> HashMap<u64, Arc<RenderImage>> {
    let chunk = ids.len().div_ceil(ICON_DOWNLOADS).max(1);
    std::thread::scope(|scope| {
        ids.chunks(chunk)
            .map(|ids| {
                scope.spawn(move || {
                    ids.iter()
                        .filter_map(|&id| Some((id, decode_icon(&cache::read_icon(id)?)?)))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .flatten()
            .collect()
    })
}

/// Downloads and caches one batch of icons in parallel.
fn download_icons(
    client: &Client,
    icons: Vec<(u64, String)>,
) -> Vec<(u64, String, Arc<RenderImage>)> {
    std::thread::scope(|scope| {
        icons
            .iter()
            .map(|(id, url)| {
                scope.spawn(move || {
                    let png = client.download_image(url).ok()?;
                    let image = decode_icon(&png)?;
                    cache::write_icon(*id, &png);
                    Some((*id, url.clone(), image))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|handle| handle.join().ok().flatten())
            .collect()
    })
}
