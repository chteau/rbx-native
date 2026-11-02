//! The Creator Store's public, anonymous routes: what the store page shows
//! for an asset (`toolbox-service/v1/items/details`: name, creator, votes,
//! price, last update) and plugin search (`toolbox-service/v2/assets:search`
//! with no key). Both answer without a key (checked live, 2026-11-02),
//! unlike Open Cloud's keyed Creator Store API in `toolbox`.

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};

/// Ids per details request; the route answered 47 at once in testing.
const BATCH: usize = 50;

/// One asset as the store lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreItem {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub creator_name: String,
    pub up_votes: u64,
    pub down_votes: u64,
    /// On sale for nothing. A paid item, or one off sale, is `false`.
    pub free: bool,
    /// RFC 3339, as the store gives it; changes with every new version.
    pub updated: String,
}

#[derive(Deserialize)]
struct DetailsRaw {
    #[serde(default)]
    data: Vec<ItemRaw>,
}

#[derive(Deserialize)]
struct ItemRaw {
    asset: AssetRaw,
    creator: CreatorRaw,
    #[serde(default)]
    voting: VotingRaw,
    #[serde(rename = "fiatProduct", default)]
    product: ProductRaw,
}

#[derive(Deserialize)]
struct AssetRaw {
    id: u64,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "updatedUtc", default)]
    updated: String,
}

#[derive(Deserialize)]
struct CreatorRaw {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct VotingRaw {
    #[serde(rename = "upVotes", default)]
    up: u64,
    #[serde(rename = "downVotes", default)]
    down: u64,
}

#[derive(Deserialize, Default)]
struct ProductRaw {
    #[serde(rename = "isFree", default)]
    free: bool,
    #[serde(default)]
    published: bool,
}

#[derive(Deserialize)]
struct SearchRaw {
    #[serde(rename = "creatorStoreAssets", default)]
    assets: Vec<SearchHitRaw>,
}

#[derive(Deserialize)]
struct SearchHitRaw {
    asset: SearchAssetRaw,
}

#[derive(Deserialize)]
struct SearchAssetRaw {
    id: u64,
}

impl Client {
    /// The store's listing of each of `ids` it knows; an id it does not
    /// answer for (deleted, never on sale) is left out.
    pub fn store_items(&self, ids: &[u64]) -> Result<Vec<StoreItem>, CloudError> {
        let url = self.apis_url("/toolbox-service/v1/items/details");
        let mut items = Vec::new();
        for chunk in ids.chunks(BATCH) {
            let ids = chunk
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let response = self.get_with(&url, false, true, |req| req.query("assetIds", &ids))?;
            if !(200..300).contains(&response.status) {
                return Err(error::error_for_status(
                    &url,
                    response.status,
                    &response.headers,
                ));
            }
            items.extend(parse_details(&response.body)?);
        }
        Ok(items)
    }

    /// Plugin ids for `query`, in the store's own relevance order (an empty
    /// query lists the most popular), at most `limit`.
    pub fn search_plugins(&self, query: &str, limit: u32) -> Result<Vec<u64>, CloudError> {
        let url = self.apis_url("/toolbox-service/v2/assets:search");
        let limit = limit.to_string();
        let response = self.get_with(&url, false, true, |req| {
            req.query("searchCategoryType", "Plugin")
                .query("sortCategory", "Relevance")
                .query("query", query)
                .query("maxPageSize", &limit)
        })?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        let raw: SearchRaw = serde_json::from_slice(&response.body)?;
        Ok(raw.assets.into_iter().map(|hit| hit.asset.id).collect())
    }
}

fn parse_details(body: &[u8]) -> Result<Vec<StoreItem>, CloudError> {
    let raw: DetailsRaw = serde_json::from_slice(body)?;
    Ok(raw
        .data
        .into_iter()
        .map(|item| StoreItem {
            id: item.asset.id,
            name: item.asset.name,
            description: item.asset.description,
            creator_name: item.creator.name,
            up_votes: item.voting.up,
            down_votes: item.voting.down,
            free: item.product.free && item.product.published,
            updated: item.asset.updated,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_keep_votes_and_price() {
        let body = br#"{"data":[
            {"asset":{"id":1,"name":"Arcs","description":"d","updatedUtc":"2024-10-03T15:47:04.703Z"},
             "creator":{"id":5,"name":"Someone","type":1},
             "voting":{"upVotes":6720,"downVotes":280},
             "fiatProduct":{"published":true,"purchasable":true,"isFree":true}},
            {"asset":{"id":2,"name":"Paid"},"creator":{"name":"Other"},
             "fiatProduct":{"published":true,"isFree":false}},
            {"asset":{"id":3,"name":"Off sale"},"creator":{"name":"Other"},
             "fiatProduct":{"published":false,"isFree":true}}
        ]}"#;
        let items = parse_details(body).unwrap();
        assert_eq!(
            items[0],
            StoreItem {
                id: 1,
                name: "Arcs".into(),
                description: "d".into(),
                creator_name: "Someone".into(),
                up_votes: 6720,
                down_votes: 280,
                free: true,
                updated: "2024-10-03T15:47:04.703Z".into(),
            }
        );
        assert!(!items[1].free);
        assert!(!items[2].free);
        assert_eq!(items[1].up_votes, 0);
    }

    #[test]
    fn empty_details_are_no_items() {
        assert!(parse_details(b"{}").unwrap().is_empty());
    }
}
