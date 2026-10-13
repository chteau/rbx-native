//! The experience and place settings Open Cloud lets an API key read and
//! write: `GET`/`PATCH /cloud/v2/universes/{id}` (`universe:write`) and
//! `GET`/`PATCH /cloud/v2/universes/{id}/places/{id}`
//! (`universe.place:write`), per `reference/cloud/openapi.json` in
//! creator-docs. An update names its fields in `updateMask` and sends only
//! those, so a value nobody touched is never written back.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::client::Client;
use crate::create_asset;
use crate::error::CloudError;
use crate::universes::{parse_visibility, trailing_numeric_segment, Visibility};

#[cfg(test)]
mod tests;

/// An experience's settings as `GET /cloud/v2/universes/{id}` returns them.
/// `display_name` and `description` are read-only here: Roblox mirrors them
/// from the root place, which is where they are written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperienceSettings {
    pub display_name: String,
    pub description: String,
    pub visibility: Visibility,
    /// The `ageRating` enum's name without its prefix (`ALL`, `9_PLUS`, …);
    /// `None` when Roblox sent `AGE_RATING_UNSPECIFIED` or nothing.
    pub age_rating: Option<String>,
    pub voice_chat_enabled: bool,
    /// `None` when private servers are off.
    pub private_server_price_robux: Option<u32>,
    pub desktop_enabled: bool,
    pub mobile_enabled: bool,
    pub tablet_enabled: bool,
    pub console_enabled: bool,
    pub vr_enabled: bool,
    pub root_place_id: u64,
}

/// A place's settings as `GET /cloud/v2/universes/{u}/places/{p}` returns them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceSettings {
    pub display_name: String,
    pub description: String,
    pub server_size: u32,
    pub root: bool,
}

/// The experience fields to change; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExperienceUpdate {
    pub voice_chat_enabled: Option<bool>,
    /// `Some(None)` turns private servers off (masked and sent as `null`).
    pub private_server_price_robux: Option<Option<u32>>,
    pub desktop_enabled: Option<bool>,
    pub mobile_enabled: Option<bool>,
    pub tablet_enabled: Option<bool>,
    pub console_enabled: Option<bool>,
    pub vr_enabled: Option<bool>,
}

/// The place fields to change; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaceUpdate {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub server_size: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExperienceRaw {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    visibility: String,
    #[serde(default)]
    age_rating: Option<String>,
    #[serde(default)]
    voice_chat_enabled: bool,
    #[serde(default)]
    private_server_price_robux: Option<u32>,
    #[serde(default)]
    desktop_enabled: bool,
    #[serde(default)]
    mobile_enabled: bool,
    #[serde(default)]
    tablet_enabled: bool,
    #[serde(default)]
    console_enabled: bool,
    #[serde(default)]
    vr_enabled: bool,
    #[serde(default)]
    root_place: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaceRaw {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    server_size: u32,
    #[serde(default)]
    root: bool,
}

pub(crate) fn parse_experience(value: Value) -> Result<ExperienceSettings, CloudError> {
    let raw: ExperienceRaw = serde_json::from_value(value)?;
    let root_place_id = trailing_numeric_segment(&raw.root_place).ok_or_else(|| {
        CloudError::UnexpectedShape(format!("unparseable rootPlace {:?}", raw.root_place))
    })?;
    let age_rating = raw
        .age_rating
        .as_deref()
        .map(|a| a.strip_prefix("AGE_RATING_").unwrap_or(a))
        .filter(|a| !a.is_empty() && *a != "UNSPECIFIED")
        .map(str::to_owned);
    Ok(ExperienceSettings {
        display_name: raw.display_name,
        description: raw.description,
        visibility: parse_visibility(&raw.visibility),
        age_rating,
        voice_chat_enabled: raw.voice_chat_enabled,
        private_server_price_robux: raw.private_server_price_robux,
        desktop_enabled: raw.desktop_enabled,
        mobile_enabled: raw.mobile_enabled,
        tablet_enabled: raw.tablet_enabled,
        console_enabled: raw.console_enabled,
        vr_enabled: raw.vr_enabled,
        root_place_id,
    })
}

pub(crate) fn parse_place(value: Value) -> Result<PlaceSettings, CloudError> {
    let raw: PlaceRaw = serde_json::from_value(value)?;
    Ok(PlaceSettings {
        display_name: raw.display_name,
        description: raw.description,
        server_size: raw.server_size,
        root: raw.root,
    })
}

fn finish(mask: Vec<&'static str>, body: Map<String, Value>) -> (String, Map<String, Value>) {
    (mask.join(","), body)
}

impl ExperienceUpdate {
    pub fn is_empty(&self) -> bool {
        self.request().0.is_empty()
    }

    /// The `updateMask` value and the JSON body carrying exactly those fields.
    pub(crate) fn request(&self) -> (String, Map<String, Value>) {
        let mut mask = Vec::new();
        let mut body = Map::new();
        let mut put = |name: &'static str, value: Value| {
            mask.push(name);
            body.insert(name.to_owned(), value);
        };
        if let Some(v) = self.voice_chat_enabled {
            put("voiceChatEnabled", Value::Bool(v));
        }
        if let Some(v) = self.private_server_price_robux {
            put(
                "privateServerPriceRobux",
                v.map_or(Value::Null, Value::from),
            );
        }
        if let Some(v) = self.desktop_enabled {
            put("desktopEnabled", Value::Bool(v));
        }
        if let Some(v) = self.mobile_enabled {
            put("mobileEnabled", Value::Bool(v));
        }
        if let Some(v) = self.tablet_enabled {
            put("tabletEnabled", Value::Bool(v));
        }
        if let Some(v) = self.console_enabled {
            put("consoleEnabled", Value::Bool(v));
        }
        if let Some(v) = self.vr_enabled {
            put("vrEnabled", Value::Bool(v));
        }
        finish(mask, body)
    }
}

impl PlaceUpdate {
    pub fn is_empty(&self) -> bool {
        self.request().0.is_empty()
    }

    /// The `updateMask` value and the JSON body carrying exactly those fields.
    pub(crate) fn request(&self) -> (String, Map<String, Value>) {
        let mut mask = Vec::new();
        let mut body = Map::new();
        let mut put = |name: &'static str, value: Value| {
            mask.push(name);
            body.insert(name.to_owned(), value);
        };
        if let Some(v) = &self.display_name {
            put("displayName", Value::from(v.as_str()));
        }
        if let Some(v) = &self.description {
            put("description", Value::from(v.as_str()));
        }
        if let Some(v) = self.server_size {
            put("serverSize", Value::from(v));
        }
        finish(mask, body)
    }
}

fn universe_url(universe_id: u64) -> String {
    format!("https://apis.roblox.com/cloud/v2/universes/{universe_id}")
}

fn place_url(universe_id: u64, place_id: u64) -> String {
    format!("https://apis.roblox.com/cloud/v2/universes/{universe_id}/places/{place_id}")
}

impl Client {
    fn get_json(&self, url: &str) -> Result<Value, CloudError> {
        let response = self.get_raw(url, true, true)?;
        create_asset::answer(url, response)
    }

    fn patch_json(
        &self,
        url: &str,
        mask: &str,
        body: &Map<String, Value>,
    ) -> Result<Value, CloudError> {
        let full = format!("{url}?updateMask={mask}");
        let response = self.patch_json_raw(&full, body)?;
        create_asset::answer(&full, response)
    }

    pub fn experience_settings(&self, universe_id: u64) -> Result<ExperienceSettings, CloudError> {
        parse_experience(self.get_json(&universe_url(universe_id))?)
    }

    /// Writes `update` and returns the settings as Roblox now has them. An
    /// empty update is not sent: it reads instead. The PATCH reply is only
    /// guaranteed to carry the updated fields, so a fresh GET supplies the rest.
    pub fn update_experience_settings(
        &self,
        universe_id: u64,
        update: &ExperienceUpdate,
    ) -> Result<ExperienceSettings, CloudError> {
        if update.is_empty() {
            return self.experience_settings(universe_id);
        }
        let (mask, body) = update.request();
        self.patch_json(&universe_url(universe_id), &mask, &body)?;
        self.experience_settings(universe_id)
    }

    pub fn place_settings(
        &self,
        universe_id: u64,
        place_id: u64,
    ) -> Result<PlaceSettings, CloudError> {
        parse_place(self.get_json(&place_url(universe_id, place_id))?)
    }

    /// Writes `update` and returns the place as Roblox now has it. An empty
    /// update is not sent: it reads instead. As with the experience, the
    /// PATCH reply may hold only the updated fields, so a fresh GET follows.
    pub fn update_place_settings(
        &self,
        universe_id: u64,
        place_id: u64,
        update: &PlaceUpdate,
    ) -> Result<PlaceSettings, CloudError> {
        if update.is_empty() {
            return self.place_settings(universe_id, place_id);
        }
        let (mask, body) = update.request();
        self.patch_json(&place_url(universe_id, place_id), &mask, &body)?;
        self.place_settings(universe_id, place_id)
    }
}
