//! An experience's icon through the key: `legacy-game-internationalization`'s
//! game-icon routes (scope `legacy-universe:manage`), and the moderation
//! state of an image uploaded ahead of time through the Assets API. See
//! `docs/research/update-planning-icon-api.md` for what the spec does and
//! does not promise; none of this has been sent to Roblox from here yet.
//!
//! Never call [`Client::set_game_icon`] or [`Client::upload_icon_image`]
//! from a test: the first changes a live experience, the second creates an
//! asset. Request building and response parsing are tested instead.

use serde::Deserialize;
use serde_json::Value;

use crate::client::Client;
use crate::create_asset::{answer, boundary, ModelFile};
use crate::error::{self, CloudError};
use crate::universes::Owner;

/// The game-icon route of one universe.
fn icons_path(universe_id: u64) -> String {
    format!("/legacy-game-internationalization/v1/game-icon/games/{universe_id}")
}

fn set_path(universe_id: u64, language: &str) -> String {
    format!("{}/language-codes/{language}", icons_path(universe_id))
}

/// How Roblox has judged an image, from either the icon list or an asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
pub enum Moderation {
    Reviewing,
    Approved,
    Rejected,
    /// `UnAvailable` or `Error` on the icon list.
    Unavailable,
}

impl Moderation {
    /// Both APIs' spellings: the icon list's `PendingReview`, the asset's
    /// `Reviewing`, and the guide's `MODERATION_STATE_APPROVED`.
    fn parse(state: &str) -> Moderation {
        let state = state.to_ascii_lowercase();
        if state.contains("approved") {
            Moderation::Approved
        } else if state.contains("rejected") {
            Moderation::Rejected
        } else if state.contains("review") || state.is_empty() {
            Moderation::Reviewing
        } else {
            Moderation::Unavailable
        }
    }
}

/// One language's icon, as the icon list returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameIcon {
    pub image_id: String,
    pub image_url: Option<String>,
    pub state: Moderation,
    pub language: String,
}

#[derive(Deserialize)]
struct IconsRaw {
    #[serde(default)]
    data: Vec<IconRaw>,
}

#[derive(Deserialize)]
struct IconRaw {
    #[serde(rename = "imageId", default)]
    image_id: String,
    #[serde(rename = "imageUrl", default)]
    image_url: Option<String>,
    #[serde(default)]
    state: String,
    #[serde(rename = "languageCode", default)]
    language: String,
}

fn parse_icons(body: &[u8]) -> Result<Vec<GameIcon>, CloudError> {
    let raw: IconsRaw = serde_json::from_slice(body)?;
    Ok(raw
        .data
        .into_iter()
        .map(|icon| GameIcon {
            image_id: icon.image_id,
            image_url: icon.image_url.filter(|url| !url.is_empty()),
            state: Moderation::parse(&icon.state),
            language: icon.language,
        })
        .collect())
}

fn parse_moderation(body: &Value) -> Moderation {
    Moderation::parse(
        body.pointer("/moderationResult/moderationState")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )
}

/// The icon POST's body: one `Files` part, as the spec's shared
/// `postV1Badges_badgeid_iconsLanguageCodes_languagecode_` body names it.
fn icon_multipart(png: &[u8], boundary: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(png.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"Files\"; filename=\"icon.png\"\r\n\
             Content-Type: image/png\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

impl Client {
    /// Every language's icon of `universe_id`, with its moderation state.
    pub fn game_icons(&self, universe_id: u64) -> Result<Vec<GameIcon>, CloudError> {
        let url = self.apis_url(&icons_path(universe_id));
        let response = self.get_raw(&url, true, true)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        parse_icons(&response.body)
    }

    /// Makes `png` the experience's icon for `language`. Not retried: a
    /// failed post may still have landed.
    pub fn set_game_icon(
        &self,
        universe_id: u64,
        language: &str,
        png: &[u8],
    ) -> Result<(), CloudError> {
        let boundary = boundary(png);
        let body = icon_multipart(png, &boundary);
        let url = self.apis_url(&set_path(universe_id, language));
        let content_type = format!("multipart/form-data; boundary={boundary}");
        let response = self.post_bytes_raw(&url, &content_type, &body)?;
        answer(&url, response).map(|_| ())
    }

    /// Uploads `png` as a `Decal` owned by `owner` so Roblox reviews it
    /// before it is ever the icon. Returns the asset id.
    pub fn upload_icon_image(
        &self,
        name: &str,
        owner: Owner,
        png: &[u8],
    ) -> Result<u64, CloudError> {
        let file = ModelFile {
            name: "icon.png",
            content_type: "image/png",
            bytes: png,
        };
        self.create_asset(
            "Decal",
            name,
            "Update planning countdown icon",
            owner,
            &file,
        )
    }

    /// The review state of an image [`upload_icon_image`](Self::upload_icon_image) made.
    pub fn image_moderation(&self, asset_id: u64) -> Result<Moderation, CloudError> {
        let url = self.apis_url(&format!(
            "/assets/v1/assets/{asset_id}?readMask=moderationResult"
        ));
        let response = self.get_raw(&url, true, true)?;
        Ok(parse_moderation(&answer(&url, response)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_follow_the_spec() {
        assert_eq!(
            set_path(42, "en"),
            "/legacy-game-internationalization/v1/game-icon/games/42/language-codes/en"
        );
    }

    #[test]
    fn multipart_has_one_files_part() {
        let body = icon_multipart(b"PNGDATA", "XyZ");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--XyZ\r\nContent-Disposition: form-data; name=\"Files\""));
        assert!(text.contains("Content-Type: image/png\r\n\r\nPNGDATA\r\n--XyZ--\r\n"));
        assert_eq!(text.matches("--XyZ").count(), 2);
    }

    #[test]
    fn icon_list_parses_every_state() {
        let body = br#"{"data":[
            {"imageId":"1","imageUrl":"https://t/1","state":"Approved","languageCode":"en"},
            {"imageId":"2","imageUrl":"","state":"PendingReview","languageCode":"fr"},
            {"imageId":"3","state":"Rejected","languageCode":"de"},
            {"imageId":"4","state":"UnAvailable","languageCode":"es"}]}"#;
        let icons = parse_icons(body).unwrap();
        let states: Vec<_> = icons.iter().map(|i| i.state).collect();
        assert_eq!(
            states,
            [
                Moderation::Approved,
                Moderation::Reviewing,
                Moderation::Rejected,
                Moderation::Unavailable
            ]
        );
        assert_eq!(icons[0].image_url.as_deref(), Some("https://t/1"));
        assert_eq!(icons[1].image_url, None);
        assert_eq!(icons[1].language, "fr");
    }

    #[test]
    fn asset_moderation_takes_either_spelling() {
        let of = |state: &str| {
            parse_moderation(&serde_json::json!({"moderationResult": {"moderationState": state}}))
        };
        assert_eq!(of("Reviewing"), Moderation::Reviewing);
        assert_eq!(of("MODERATION_STATE_APPROVED"), Moderation::Approved);
        assert_eq!(of("Rejected"), Moderation::Rejected);
        assert_eq!(
            parse_moderation(&serde_json::json!({})),
            Moderation::Reviewing
        );
    }
}
