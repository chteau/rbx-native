//! Memory store calls: sorted maps and queues.

use serde_json::{json, Value};

use super::{answer, list_of, next_token, segment, Page};
use crate::client::Method;
use crate::{Client, CloudError};

const MAP_READ: &str = "memory-store.sorted-map:read";
const MAP_WRITE: &str = "memory-store.sorted-map:write";
const QUEUE_ADD: &str = "memory-store.queue:add";
const QUEUE_READ: &str = "memory-store.queue:dequeue";
const QUEUE_DISCARD: &str = "memory-store.queue:discard";

#[derive(Debug, Clone, PartialEq)]
pub enum SortKey {
    Text(String),
    Number(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapItem {
    pub key: String,
    pub value: Value,
    pub etag: String,
    pub expire_time: Option<String>,
    pub sort_key: Option<SortKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapItemWrite {
    pub value: Value,
    pub ttl_seconds: u64,
    pub sort_key: Option<SortKey>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueueItem {
    pub id: String,
    pub data: Value,
    pub priority: f64,
    pub expire_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueueRead {
    pub read_id: Option<String>,
    pub items: Vec<QueueItem>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn parse_map_item(value: &Value, fallback_key: &str) -> MapItem {
    let sort_key = match (value.get("numericSortKey"), text(value, "stringSortKey")) {
        (Some(n), _) if n.is_number() => n.as_f64().map(SortKey::Number),
        (_, Some(s)) => Some(SortKey::Text(s)),
        _ => None,
    };
    MapItem {
        key: text(value, "id").unwrap_or_else(|| fallback_key.to_string()),
        value: value.get("value").cloned().unwrap_or(Value::Null),
        etag: text(value, "etag").unwrap_or_default(),
        expire_time: text(value, "expireTime"),
        sort_key,
    }
}

fn parse_queue_item(value: &Value) -> QueueItem {
    QueueItem {
        // Live reads carry no `id`; the path's last segment is the item's.
        id: text(value, "id")
            .or_else(|| text(value, "path")?.rsplit('/').next().map(str::to_string))
            .unwrap_or_default(),
        data: value.get("data").cloned().unwrap_or(Value::Null),
        priority: value.get("priority").and_then(Value::as_f64).unwrap_or(0.0),
        expire_time: text(value, "expireTime"),
    }
}

fn map_body(write: &MapItemWrite) -> Value {
    let mut body = json!({
        "value": write.value,
        "ttl": format!("{}s", write.ttl_seconds),
    });
    match &write.sort_key {
        Some(SortKey::Text(s)) => body["stringSortKey"] = Value::String(s.clone()),
        Some(SortKey::Number(n)) => body["numericSortKey"] = json!(n),
        None => {}
    }
    if let Some(etag) = &write.etag {
        body["etag"] = Value::String(etag.clone());
    }
    body
}

impl Client {
    fn map_items_url(&self, universe_id: u64, map: &str) -> String {
        self.apis_url(&format!(
            "/cloud/v2/universes/{universe_id}/memory-store/sorted-maps/{}/items",
            segment(map)
        ))
    }

    fn queue_items_url(&self, universe_id: u64, queue: &str) -> String {
        self.apis_url(&format!(
            "/cloud/v2/universes/{universe_id}/memory-store/queues/{}/items",
            segment(queue)
        ))
    }

    pub fn list_map_items(
        &self,
        universe_id: u64,
        map: &str,
        descending: bool,
        after: Option<&str>,
        page_token: Option<&str>,
    ) -> Result<Page<MapItem>, CloudError> {
        let url = self.map_items_url(universe_id, map);
        let filter = after.map(|a| {
            let op = if descending { '<' } else { '>' };
            format!(
                "id {op} \"{}\"",
                a.replace('\\', "\\\\").replace('"', "\\\"")
            )
        });
        let response = self.get_with(&url, true, true, |req| {
            let mut req = req.query("maxPageSize", "100");
            if let Some(t) = page_token {
                req = req.query("pageToken", t);
            }
            if descending {
                req = req.query("orderBy", "id desc");
            }
            if let Some(f) = &filter {
                req = req.query("filter", f.as_str());
            }
            req
        })?;
        let body = answer(&url, response, MAP_READ)?;
        let items = list_of(&body, &["items"])
            .iter()
            .map(|v| parse_map_item(v, ""))
            .collect();
        Ok(Page {
            items,
            next_page_token: next_token(&body),
        })
    }

    pub fn get_map_item(
        &self,
        universe_id: u64,
        map: &str,
        key: &str,
    ) -> Result<MapItem, CloudError> {
        let url = format!("{}/{}", self.map_items_url(universe_id, map), segment(key));
        let response = self.get_with(&url, true, true, |req| req)?;
        Ok(parse_map_item(&answer(&url, response, MAP_READ)?, key))
    }

    pub fn create_map_item(
        &self,
        universe_id: u64,
        map: &str,
        key: &str,
        write: &MapItemWrite,
    ) -> Result<MapItem, CloudError> {
        let url = format!(
            "{}?id={}",
            self.map_items_url(universe_id, map),
            segment(key)
        );
        let response = self.send_json(Method::Post, &url, Some(&map_body(write)))?;
        Ok(parse_map_item(&answer(&url, response, MAP_WRITE)?, key))
    }

    pub fn update_map_item(
        &self,
        universe_id: u64,
        map: &str,
        key: &str,
        write: &MapItemWrite,
    ) -> Result<MapItem, CloudError> {
        let url = format!("{}/{}", self.map_items_url(universe_id, map), segment(key));
        let response = self.send_json(Method::Patch, &url, Some(&map_body(write)))?;
        Ok(parse_map_item(&answer(&url, response, MAP_WRITE)?, key))
    }

    pub fn delete_map_item(
        &self,
        universe_id: u64,
        map: &str,
        key: &str,
    ) -> Result<(), CloudError> {
        let url = format!("{}/{}", self.map_items_url(universe_id, map), segment(key));
        let response = self.send_json(Method::Delete, &url, None)?;
        answer(&url, response, MAP_WRITE).map(|_| ())
    }

    pub fn read_queue(
        &self,
        universe_id: u64,
        queue: &str,
        count: u32,
        invisibility_seconds: u32,
    ) -> Result<QueueRead, CloudError> {
        let url = format!("{}:read", self.queue_items_url(universe_id, queue));
        let response = self.get_with(&url, true, true, |req| {
            req.query("count", count.to_string())
                .query("allOrNothing", "false")
                .query("invisibilityWindow", format!("{invisibility_seconds}s"))
        })?;
        // What Roblox actually answers differs from its OpenAPI document:
        // `queueItems` and `id` rather than `items` and `readId`, and an
        // empty queue is a 404 "Queue items not found." rather than an empty
        // list. Both spellings are read; the 404 is an empty read.
        let body = match answer(&url, response, QUEUE_READ) {
            Err(CloudError::Refused { status: 404, .. } | CloudError::Http { status: 404, .. }) => {
                return Ok(QueueRead {
                    read_id: None,
                    items: Vec::new(),
                })
            }
            other => other?,
        };
        Ok(QueueRead {
            read_id: text(&body, "readId").or_else(|| text(&body, "id")),
            items: list_of(&body, &["queueItems", "items"])
                .iter()
                .map(parse_queue_item)
                .collect(),
        })
    }

    pub fn discard_queue_items(
        &self,
        universe_id: u64,
        queue: &str,
        read_id: &str,
    ) -> Result<(), CloudError> {
        let url = format!("{}:discard", self.queue_items_url(universe_id, queue));
        let body = json!({ "readId": read_id });
        let response = self.send_json(Method::Post, &url, Some(&body))?;
        answer(&url, response, QUEUE_DISCARD).map(|_| ())
    }

    pub fn add_queue_item(
        &self,
        universe_id: u64,
        queue: &str,
        data: &Value,
        priority: f64,
        ttl_seconds: u64,
    ) -> Result<QueueItem, CloudError> {
        let url = self.queue_items_url(universe_id, queue);
        let body = json!({
            "data": data,
            "priority": priority,
            "ttl": format!("{ttl_seconds}s"),
        });
        let response = self.send_json(Method::Post, &url, Some(&body))?;
        Ok(parse_queue_item(&answer(&url, response, QUEUE_ADD)?))
    }
}
