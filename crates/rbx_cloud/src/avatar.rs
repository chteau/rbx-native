//! `GET avatar.roblox.com/v1/users/{id}/avatar`: what a user wears and how
//! their body is scaled. Public and anonymous, so it needs no key and no
//! cookie, only the user id (which the stored Open Cloud key supplies).

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};

/// One worn asset; `asset_type_id` is Roblox's `AssetType` number (8 Hat,
/// 11 Shirt, 12 Pants, 27-31 body parts, 41-47 accessories, 79 DynamicHead).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AvatarAsset {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "assetType")]
    pub asset_type: AssetType,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AssetType {
    pub id: u32,
    #[serde(default)]
    pub name: String,
}

/// The six avatar scale sliders, as Roblox stores them.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct AvatarScales {
    pub height: f64,
    pub width: f64,
    pub head: f64,
    pub depth: f64,
    pub proportion: f64,
    #[serde(rename = "bodyType")]
    pub body_type: f64,
}

/// BrickColor numbers per body part.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct AvatarBodyColors {
    #[serde(rename = "headColorId")]
    pub head: u32,
    #[serde(rename = "torsoColorId")]
    pub torso: u32,
    #[serde(rename = "rightArmColorId")]
    pub right_arm: u32,
    #[serde(rename = "leftArmColorId")]
    pub left_arm: u32,
    #[serde(rename = "rightLegColorId")]
    pub right_leg: u32,
    #[serde(rename = "leftLegColorId")]
    pub left_leg: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Avatar {
    pub scales: AvatarScales,
    /// "R15" or "R6".
    #[serde(rename = "playerAvatarType")]
    pub avatar_type: String,
    #[serde(rename = "bodyColors")]
    pub body_colors: AvatarBodyColors,
    #[serde(default)]
    pub assets: Vec<AvatarAsset>,
}

impl Avatar {
    /// Parses the endpoint's JSON body (also the mock file's format).
    pub fn from_json(body: &[u8]) -> Result<Avatar, CloudError> {
        Ok(serde_json::from_slice(body)?)
    }
}

impl Client {
    pub fn avatar(&self, user_id: u64) -> Result<Avatar, CloudError> {
        let url = format!("https://avatar.roblox.com/v1/users/{user_id}/avatar");
        let response = self.get_raw(&url, false, true)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        Avatar::from_json(&response.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &[u8] = br#"{"scales":{"height":1.0,"width":1.0,"head":1.0,"depth":0.9,"proportion":0.0,"bodyType":0.0},"playerAvatarType":"R15","bodyColors":{"headColorId":24,"torsoColorId":23,"rightArmColorId":24,"leftArmColorId":24,"rightLegColorId":119,"leftLegColorId":119},"assets":[{"id":1028606,"name":"Red Baseball Cap","assetType":{"id":8,"name":"Hat"},"currentVersionId":1},{"id":144076358,"name":"Shirt","assetType":{"id":11,"name":"Shirt"},"currentVersionId":2}],"defaultShirtApplied":false,"emotes":[]}"#;

    #[test]
    fn parses_a_real_shaped_response() {
        let avatar = Avatar::from_json(BODY).unwrap();
        assert_eq!(avatar.avatar_type, "R15");
        assert_eq!(avatar.scales.depth, 0.9);
        assert_eq!(avatar.body_colors.right_leg, 119);
        assert_eq!(avatar.assets.len(), 2);
        assert_eq!(avatar.assets[0].asset_type.id, 8);
    }

    #[test]
    fn rejects_a_body_without_scales() {
        assert!(Avatar::from_json(br#"{"playerAvatarType":"R6"}"#).is_err());
    }
}
