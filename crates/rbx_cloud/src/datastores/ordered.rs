//! Ordered data store calls.

use serde_json::{json, Value};

use super::{answer, list_of, next_token, segment, Page, StoreRef};
use crate::client::Method;
use crate::{Client, CloudError};

const READ: &str = "universe.ordered-data-store.scope.entry:read";
const WRITE: &str = "universe.ordered-data-store.scope.entry:write";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedEntry {
    pub key: String,
    pub value: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OrderedQuery {
    pub descending: bool,
    pub min: Option<i64>,
    pub max: Option<i64>,
}

fn parse_ordered(value: &Value, fallback_key: &str) -> OrderedEntry {
    OrderedEntry {
        key: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(fallback_key)
            .to_string(),
        value: value.get("value").and_then(Value::as_i64).unwrap_or(0),
    }
}

fn filter_for(query: OrderedQuery) -> Option<String> {
    match (query.min, query.max) {
        (Some(lo), Some(hi)) => Some(format!("entry >= {lo} && entry <= {hi}")),
        (Some(lo), None) => Some(format!("entry >= {lo}")),
        (None, Some(hi)) => Some(format!("entry <= {hi}")),
        (None, None) => None,
    }
}

impl Client {
    fn ordered_url(&self, at: &StoreRef) -> String {
        self.apis_url(&format!(
            "/cloud/v2/universes/{}/ordered-data-stores/{}/scopes/{}/entries",
            at.universe_id,
            segment(&at.store),
            segment(&at.scope)
        ))
    }

    fn ordered_write(
        &self,
        method: Method,
        url: &str,
        key: &str,
        body: &Value,
    ) -> Result<OrderedEntry, CloudError> {
        let response = self.send_json(method, url, Some(body))?;
        Ok(parse_ordered(&answer(url, response, WRITE)?, key))
    }

    pub fn list_ordered(
        &self,
        at: &StoreRef,
        query: OrderedQuery,
        page_token: Option<&str>,
    ) -> Result<Page<OrderedEntry>, CloudError> {
        let url = self.ordered_url(at);
        let filter = filter_for(query);
        let response = self.get_with(&url, true, true, |req| {
            let mut req = req.query("maxPageSize", "100");
            if let Some(t) = page_token {
                req = req.query("pageToken", t);
            }
            if query.descending {
                req = req.query("orderBy", "value desc");
            }
            if let Some(f) = &filter {
                req = req.query("filter", f.as_str());
            }
            req
        })?;
        let body = answer(&url, response, READ)?;
        let items = list_of(&body, &["orderedDataStoreEntries"])
            .iter()
            .map(|v| parse_ordered(v, ""))
            .collect();
        Ok(Page {
            items,
            next_page_token: next_token(&body),
        })
    }

    pub fn get_ordered(&self, at: &StoreRef, key: &str) -> Result<OrderedEntry, CloudError> {
        let url = format!("{}/{}", self.ordered_url(at), segment(key));
        let response = self.get_with(&url, true, true, |req| req)?;
        Ok(parse_ordered(&answer(&url, response, READ)?, key))
    }

    pub fn create_ordered(
        &self,
        at: &StoreRef,
        key: &str,
        value: i64,
    ) -> Result<OrderedEntry, CloudError> {
        let url = format!("{}?id={}", self.ordered_url(at), segment(key));
        self.ordered_write(Method::Post, &url, key, &json!({ "value": value }))
    }

    pub fn update_ordered(
        &self,
        at: &StoreRef,
        key: &str,
        value: i64,
    ) -> Result<OrderedEntry, CloudError> {
        let url = format!("{}/{}", self.ordered_url(at), segment(key));
        self.ordered_write(Method::Patch, &url, key, &json!({ "value": value }))
    }

    pub fn increment_ordered(
        &self,
        at: &StoreRef,
        key: &str,
        amount: i64,
    ) -> Result<OrderedEntry, CloudError> {
        let url = format!("{}/{}:increment", self.ordered_url(at), segment(key));
        self.ordered_write(Method::Post, &url, key, &json!({ "amount": amount }))
    }

    pub fn delete_ordered(&self, at: &StoreRef, key: &str) -> Result<(), CloudError> {
        let url = format!("{}/{}", self.ordered_url(at), segment(key));
        let response = self.send_json(Method::Delete, &url, None)?;
        answer(&url, response, WRITE).map(|_| ())
    }
}
