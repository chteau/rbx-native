//! `GET thumbnails.roblox.com/v1/games/icons`: an experience's icon for the
//! Home screen's grid. Anonymous, and answers for private universes too
//! (checked live, 2026-09-25), so no scope is needed. And
//! `/v1/users/avatar-headshot`: a collaborator's round headshot, as Studio
//! shows it beside the people in a Team Create session; anonymous too.
//! `/v1/games/{id}/thumbnails`: Game Settings' thumbnail strip.
//! `/v1/assets`: a Creator Store item's icon, for the plugin catalog.
//! And any `rbxthumb://` content id a game puts in an image property
//! (`Players:GetUserThumbnailAsync`'s headshots and the like).

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};

const ICONS_URL: &str = "https://thumbnails.roblox.com/v1/games/icons";
const THUMBNAILS_ROOT: &str = "https://thumbnails.roblox.com/v1/games";
const HEADSHOTS_URL: &str = "https://thumbnails.roblox.com/v1/users/avatar-headshot";
const ASSETS_URL: &str = "https://thumbnails.roblox.com/v1/assets";

/// `rbxthumb://` types: (type, thumbnails API path, its id parameter).
const THUMB_TYPES: &[(&str, &str, &str)] = &[
    ("AvatarHeadShot", "users/avatar-headshot", "userIds"),
    ("Avatar", "users/avatar", "userIds"),
    ("AvatarBust", "users/avatar-bust", "userIds"),
    ("Asset", "assets", "assetIds"),
    ("BadgeIcon", "badges/icons", "badgeIds"),
    ("BundleThumbnail", "bundles/thumbnails", "bundleIds"),
    ("GameIcon", "games/icons", "universeIds"),
    ("GamePass", "game-passes", "gamePassIds"),
    ("GroupIcon", "groups/icons", "groupIds"),
    ("Outfit", "users/outfits", "userOutfitIds"),
];

/// The endpoint's own per-request cap.
const BATCH: usize = 100;

#[derive(Deserialize)]
struct IconsRaw {
    data: Vec<IconRaw>,
}

#[derive(Deserialize)]
struct IconRaw {
    #[serde(rename = "targetId")]
    target_id: u64,
    state: String,
    #[serde(rename = "imageUrl", default)]
    image_url: Option<String>,
}

impl Client {
    /// `(universe_id, icon URL)` for every universe whose icon is ready;
    /// one still rendering or moderated is left out rather than given a
    /// placeholder, so the caller draws its own.
    pub fn game_icon_urls(&self, universe_ids: &[u64]) -> Result<Vec<(u64, String)>, CloudError> {
        let mut icons = Vec::new();
        for chunk in universe_ids.chunks(BATCH) {
            let ids = chunk
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let response = self.get_with(ICONS_URL, false, true, |req| {
                req.query("universeIds", &ids)
                    .query("size", "512x512")
                    .query("format", "Png")
                    .query("isCircular", "false")
            })?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    ICONS_URL,
                    response.status,
                    &response.headers,
                ));
            }
            icons.extend(parse(&response.body)?);
        }
        Ok(icons)
    }

    /// `(user_id, headshot URL)` for every user whose 48×48 circular
    /// headshot is ready; the rest are left out for the caller's own
    /// placeholder.
    pub fn user_headshot_urls(&self, user_ids: &[u64]) -> Result<Vec<(u64, String)>, CloudError> {
        let mut headshots = Vec::new();
        for chunk in user_ids.chunks(BATCH) {
            let ids = chunk
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let response = self.get_with(HEADSHOTS_URL, false, true, |req| {
                req.query("userIds", &ids)
                    .query("size", "48x48")
                    .query("format", "Png")
                    .query("isCircular", "true")
            })?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    HEADSHOTS_URL,
                    response.status,
                    &response.headers,
                ));
            }
            headshots.extend(parse(&response.body)?);
        }
        Ok(headshots)
    }

    /// `(thumbnail id, URL)` for each of `ids` (an experience's media ids,
    /// see `experience_thumbnails`) whose 768×432 render is ready. Anonymous,
    /// like the icons.
    pub fn game_thumbnail_urls(
        &self,
        universe_id: u64,
        ids: &[u64],
    ) -> Result<Vec<(u64, String)>, CloudError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{THUMBNAILS_ROOT}/{universe_id}/thumbnails");
        let ids = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
        let response = self.get_with(&url, false, true, |req| {
            req.query("thumbnailIds", &ids)
                .query("size", "768x432")
                .query("format", "Png")
        })?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        parse(&response.body)
    }

    /// `(asset_id, icon URL)` for each asset whose 150×150 icon is ready,
    /// as the Creator Store shows it; anonymous like the rest.
    pub fn asset_icon_urls(&self, asset_ids: &[u64]) -> Result<Vec<(u64, String)>, CloudError> {
        let mut icons = Vec::new();
        for chunk in asset_ids.chunks(BATCH) {
            let ids = chunk
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let response = self.get_with(ASSETS_URL, false, true, |req| {
                req.query("assetIds", &ids)
                    .query("size", "150x150")
                    .query("format", "Png")
                    .query("isCircular", "false")
            })?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    ASSETS_URL,
                    response.status,
                    &response.headers,
                ));
            }
            icons.extend(parse(&response.body)?);
        }
        Ok(icons)
    }

    /// The PNG an `rbxthumb://` content id names; `spec` is what follows the
    /// scheme (`type=AvatarHeadShot&id=1&w=150&h=150`). Anonymous. A
    /// thumbnail still rendering is asked for again a few times, as the
    /// client keeps a placeholder until it's ready.
    pub fn rbxthumb(&self, spec: &str) -> Result<Vec<u8>, CloudError> {
        let field = |name: &str| {
            spec.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                key.eq_ignore_ascii_case(name).then_some(value)
            })
        };
        let unknown = || CloudError::UnexpectedShape(format!("unsupported thumbnail: {spec}"));
        let kind = field("type").ok_or_else(unknown)?;
        let (path, ids) = THUMB_TYPES
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(kind))
            .map(|(_, path, ids)| (*path, *ids))
            .ok_or_else(unknown)?;
        let id = field("id").ok_or_else(unknown)?;
        let size = format!(
            "{}x{}",
            field("w").unwrap_or("150"),
            field("h").unwrap_or("150")
        );
        let url = format!("https://thumbnails.roblox.com/v1/{path}");
        for attempt in 0..4 {
            let response = self.get_with(&url, false, true, |req| {
                req.query(ids, id)
                    .query("size", &size)
                    .query("format", "Png")
                    .query("isCircular", "false")
            })?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    &url,
                    response.status,
                    &response.headers,
                ));
            }
            if let Some((_, image)) = parse(&response.body)?.into_iter().next() {
                return self.download_image(&image);
            }
            std::thread::sleep(std::time::Duration::from_millis(500 * (attempt + 1)));
        }
        Err(CloudError::UnexpectedShape(format!(
            "thumbnail not ready: {spec}"
        )))
    }

    /// The PNG behind one of [`game_icon_urls`](Self::game_icon_urls)' URLs.
    pub fn download_image(&self, url: &str) -> Result<Vec<u8>, CloudError> {
        let response = self.get_raw(url, false, true)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                url,
                response.status,
                &response.headers,
            ));
        }
        Ok(response.body)
    }
}

fn parse(body: &[u8]) -> Result<Vec<(u64, String)>, CloudError> {
    let raw: IconsRaw = serde_json::from_slice(body)?;
    Ok(raw
        .data
        .into_iter()
        .filter(|icon| icon.state == "Completed")
        .filter_map(|icon| Some((icon.target_id, icon.image_url?)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_completed_icons_only() {
        let body = br#"{"data":[{"targetId":1,"state":"Completed","imageUrl":"https://t7.rbxcdn.com/a","version":"TN3"},{"targetId":2,"state":"Pending","imageUrl":null,"version":"TN3"},{"targetId":3,"state":"Blocked","imageUrl":"https://t7.rbxcdn.com/b"}]}"#;
        assert_eq!(
            parse(body).unwrap(),
            vec![(1, "https://t7.rbxcdn.com/a".to_string())]
        );
    }
}
