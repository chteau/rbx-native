//! MemoryStoreService's sorted maps and queues and MessagingService's
//! `PublishAsync`, live through Open Cloud. Values travel as the JSON text
//! the Luau side encodes. Under the read-only guard a sorted map write is
//! kept in the run (reads see it) and queues and publishing stay local
//! (the Luau side never sends them).

use rbx_cloud::{MapItem, MapItemWrite, SortKey};
use serde_json::{json, Value};

use super::{not_found, roblox_error, LiveStores};

/// Items listed for a range or a size: ten pages of a hundred.
const LIST_PAGES: usize = 10;

impl LiveStores {
    /// `op` when it's a memory store or messaging call, `None` otherwise.
    pub(super) fn memory(&self, op: &str, args: &[Value]) -> Option<Result<Value, String>> {
        let text = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or_default();
        let number = |i: usize| args.get(i).and_then(Value::as_f64).unwrap_or(0.0);
        Some(match op {
            "map_get" => self.map_get(text(0), text(1)),
            "map_set" => self.map_set(
                text(0),
                text(1),
                text(2),
                number(3) as u64,
                args.get(4).cloned().unwrap_or(Value::Null),
            ),
            "map_remove" => self.map_remove(text(0), text(1)),
            "map_list" => self.map_list(text(0)),
            "queue_add" | "queue_read" | "queue_remove" | "publish" if self.read_only => {
                Err("the read-only guard keeps queues and messages in the run".into())
            }
            "queue_add" => self
                .client
                .add_queue_item(
                    self.universe,
                    text(0),
                    &parse(text(1)),
                    number(3),
                    number(2) as u64,
                )
                .map(|_| Value::Null)
                .map_err(|e| roblox_error(&e)),
            "queue_read" => self
                .client
                .read_queue(self.universe, text(0), number(1) as u32, number(2) as u32)
                .map(|read| {
                    json!({
                        "id": read.read_id,
                        "values": read.items.iter().map(|i| i.data.to_string()).collect::<Vec<_>>(),
                    })
                })
                .map_err(|e| roblox_error(&e)),
            "queue_remove" => match self
                .client
                .discard_queue_items(self.universe, text(0), text(1))
            {
                Ok(()) => Ok(Value::Null),
                Err(err) if not_found(&err) => Ok(Value::Null),
                Err(err) => Err(roblox_error(&err)),
            },
            "publish" => self
                .client
                .publish_message(self.universe, text(0), text(1))
                .map(|()| Value::Null)
                .map_err(|e| roblox_error(&e)),
            _ => return None,
        })
    }

    fn map_bucket(map: &str) -> String {
        format!("m{}{map}", super::SEP)
    }

    fn map_get(&self, map: &str, key: &str) -> Result<Value, String> {
        if let Some(kept) = self.kept(&Self::map_bucket(map), key) {
            return Ok(kept);
        }
        match self.client.get_map_item(self.universe, map, key) {
            Ok(item) => Ok(item_json(&item)),
            Err(err) if not_found(&err) => Ok(Value::Null),
            Err(err) => Err(roblox_error(&err)),
        }
    }

    /// Whether the key was there before.
    fn map_set(
        &self,
        map: &str,
        key: &str,
        value: &str,
        ttl: u64,
        sort_key: Value,
    ) -> Result<Value, String> {
        if self.read_only {
            let existed = !self.map_get(map, key)?.is_null();
            let item = json!({ "key": key, "value": value, "sortKey": sort_key });
            self.keep(&Self::map_bucket(map), key, item);
            return Ok(Value::Bool(existed));
        }
        let write = MapItemWrite {
            value: parse(value),
            ttl_seconds: ttl.max(1),
            sort_key: match sort_key {
                Value::Number(n) => n.as_f64().map(SortKey::Number),
                Value::String(s) => Some(SortKey::Text(s)),
                _ => None,
            },
            etag: None,
        };
        match self.client.update_map_item(self.universe, map, key, &write) {
            Ok(_) => Ok(Value::Bool(true)),
            Err(err) if not_found(&err) => self
                .client
                .create_map_item(self.universe, map, key, &write)
                .map(|_| Value::Bool(false))
                .map_err(|e| roblox_error(&e)),
            Err(err) => Err(roblox_error(&err)),
        }
    }

    fn map_remove(&self, map: &str, key: &str) -> Result<Value, String> {
        if self
            .keep(&Self::map_bucket(map), key, Value::Null)
            .is_some()
        {
            return Ok(Value::Null);
        }
        match self.client.delete_map_item(self.universe, map, key) {
            Ok(()) => Ok(Value::Null),
            Err(err) if not_found(&err) => Ok(Value::Null),
            Err(err) => Err(roblox_error(&err)),
        }
    }

    /// Every item of the map (as far as [`LIST_PAGES`] reach), with what
    /// the guard kept laid over them; the Luau side sorts and bounds.
    fn map_list(&self, map: &str) -> Result<Value, String> {
        let mut items: Vec<Value> = Vec::new();
        let mut token = None;
        for _ in 0..LIST_PAGES {
            let page =
                match self
                    .client
                    .list_map_items(self.universe, map, false, None, token.as_deref())
                {
                    Ok(page) => page,
                    Err(err) if not_found(&err) => break,
                    Err(err) => return Err(roblox_error(&err)),
                };
            items.extend(page.items.iter().map(item_json));
            token = page.next_page_token.filter(|t| !t.is_empty());
            if token.is_none() {
                break;
            }
        }
        let bucket = Self::map_bucket(map);
        let written = self.written.lock().unwrap_or_else(|e| e.into_inner());
        let kept: Vec<(&String, &Value)> = written
            .iter()
            .filter(|((b, _), _)| *b == bucket)
            .map(|((_, key), item)| (key, item))
            .collect();
        items.retain(|item| !kept.iter().any(|(key, _)| item["key"] == key.as_str()));
        items.extend(
            kept.into_iter()
                .filter(|(_, i)| !i.is_null())
                .map(|(_, i)| i.clone()),
        );
        Ok(Value::Array(items))
    }
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

fn item_json(item: &MapItem) -> Value {
    json!({
        "key": item.key,
        "value": item.value.to_string(),
        "sortKey": match &item.sort_key {
            Some(SortKey::Number(n)) => json!(n),
            Some(SortKey::Text(s)) => json!(s),
            None => Value::Null,
        },
    })
}
