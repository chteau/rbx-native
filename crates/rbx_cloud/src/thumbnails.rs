//! `GET thumbnails.roblox.com/v1/games/icons`: an experience's icon for the
//! Home screen's grid. Anonymous, and answers for private universes too
//! (checked live, 2026-09-25), so no scope is needed.

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};

const ICONS_URL: &str = "https://thumbnails.roblox.com/v1/games/icons";

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
