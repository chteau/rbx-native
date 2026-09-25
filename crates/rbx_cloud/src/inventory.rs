//! `GET /cloud/v2/users/{id}/inventory-items` filtered to `CREATED_PLACE`:
//! every place a user created, private ones included — the only API-key
//! route to a user's private experiences on an unrestricted key. Needs
//! `user.inventory-item:read` on a key the user themself created.

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};

#[derive(Deserialize)]
struct PageRaw {
    #[serde(rename = "inventoryItems", default)]
    items: Vec<ItemRaw>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct ItemRaw {
    #[serde(rename = "assetDetails")]
    asset_details: Option<AssetDetailsRaw>,
}

#[derive(Deserialize)]
struct AssetDetailsRaw {
    #[serde(rename = "assetId")]
    asset_id: String,
}

impl Client {
    /// The ids of every place `user_id` created, oldest first — root places
    /// and sub-places alike.
    pub fn created_places(&self, user_id: u64) -> Result<Vec<u64>, CloudError> {
        let url = format!("https://apis.roblox.com/cloud/v2/users/{user_id}/inventory-items");
        let mut places = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let response = self.get_with(&url, true, true, |req| {
                let req = req
                    .query("maxPageSize", "100")
                    .query("filter", "inventoryItemAssetTypes=CREATED_PLACE");
                match &token {
                    Some(t) => req.query("pageToken", t),
                    None => req,
                }
            })?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    &url,
                    response.status,
                    &response.headers,
                ));
            }
            let (ids, next) = parse(&response.body)?;
            places.extend(ids);
            match next {
                Some(next) => token = Some(next),
                None => break,
            }
        }
        Ok(places)
    }
}

fn parse(body: &[u8]) -> Result<(Vec<u64>, Option<String>), CloudError> {
    let page: PageRaw = serde_json::from_slice(body)?;
    let ids = page
        .items
        .into_iter()
        .filter_map(|item| item.asset_details?.asset_id.parse().ok())
        .collect();
    Ok((ids, page.next_page_token.filter(|t| !t.is_empty())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_created_place_page() {
        // users/156, 2026-09-25, trimmed to one item.
        let body = br#"{"inventoryItems":[{"path":"users/156/inventory-items/VVNFUl9BU1NFVF9JRD0xNDU3OTY2","assetDetails":{"assetId":"1501","inventoryItemAssetType":"CREATED_PLACE","instanceId":"1457966"},"addTime":"2008-03-01T07:21:42.630Z"}],"nextPageToken":"djEv"}"#;
        assert_eq!(parse(body).unwrap(), (vec![1501], Some("djEv".to_string())));
        assert_eq!(
            parse(br#"{"inventoryItems":[],"nextPageToken":""}"#).unwrap(),
            (vec![], None)
        );
    }
}
