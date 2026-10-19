//! Standard data store calls.

use serde_json::{json, Map, Value};

use super::{
    answer, parse_entry, parse_refs, parse_revisions, parse_stores, segment, DataStoreInfo, Entry,
    EntryRef, EntryWrite, Page, Revision, StoreRef,
};
use crate::client::Method;
use crate::{Client, CloudError};

const LIST_STORES: &str = "universe-datastores.control:list";
const LIST_ENTRIES: &str = "universe-datastores.objects:list";
const READ: &str = "universe-datastores.objects:read";
const CREATE: &str = "universe-datastores.objects:create";
const UPDATE: &str = "universe-datastores.objects:update";
const DELETE: &str = "universe-datastores.objects:delete";
const LIST_REVISIONS: &str = "universe-datastores.versions:list";

fn user_paths(users: &[u64]) -> Vec<String> {
    users.iter().map(|u| format!("users/{u}")).collect()
}

fn filter_for(prefix: &str) -> Option<String> {
    (!prefix.is_empty()).then(|| format!("id.startsWith(\"{}\")", prefix.replace('"', "\\\"")))
}

impl Client {
    fn entries_url(&self, at: &StoreRef) -> String {
        self.apis_url(&format!(
            "/cloud/v2/universes/{}/data-stores/{}/scopes/{}/entries",
            at.universe_id,
            segment(&at.store),
            segment(&at.scope)
        ))
    }

    fn entry_url(&self, at: &StoreRef, key: &str) -> String {
        format!("{}/{}", self.entries_url(at), segment(key))
    }

    fn paged_get(
        &self,
        url: &str,
        scope: &str,
        max: u32,
        filter: Option<String>,
        page_token: Option<&str>,
        extra: Option<(&str, &str)>,
    ) -> Result<Value, CloudError> {
        let response = self.get_with(url, true, true, |req| {
            let mut req = req.query("maxPageSize", max.to_string());
            if let Some(t) = page_token {
                req = req.query("pageToken", t);
            }
            if let Some(f) = &filter {
                req = req.query("filter", f.as_str());
            }
            if let Some((k, v)) = extra {
                req = req.query(k, v);
            }
            req
        })?;
        answer(url, response, scope)
    }

    pub fn list_data_stores(
        &self,
        universe_id: u64,
        prefix: &str,
        show_deleted: bool,
        page_token: Option<&str>,
    ) -> Result<Page<DataStoreInfo>, CloudError> {
        let url = self.apis_url(&format!("/cloud/v2/universes/{universe_id}/data-stores"));
        let extra = show_deleted.then_some(("showDeleted", "true"));
        let body = self.paged_get(
            &url,
            LIST_STORES,
            100,
            filter_for(prefix),
            page_token,
            extra,
        )?;
        parse_stores(&body)
    }

    pub fn list_entries(
        &self,
        at: &StoreRef,
        prefix: &str,
        page_token: Option<&str>,
    ) -> Result<Page<EntryRef>, CloudError> {
        let url = self.entries_url(at);
        let body = self.paged_get(
            &url,
            LIST_ENTRIES,
            256,
            filter_for(prefix),
            page_token,
            None,
        )?;
        parse_refs(&body, &at.scope)
    }

    fn fetch_entry(&self, at: &StoreRef, key: &str, url: String) -> Result<Entry, CloudError> {
        let response = self.get_with(&url, true, true, |req| req)?;
        parse_entry(answer(&url, response, READ)?, &at.scope, key)
    }

    pub fn get_entry(&self, at: &StoreRef, key: &str) -> Result<Entry, CloudError> {
        let mut url = self.entry_url(at, key);
        if key.contains('@') {
            url.push_str("@latest");
        }
        self.fetch_entry(at, key, url)
    }

    pub fn get_entry_revision(
        &self,
        at: &StoreRef,
        key: &str,
        revision_id: &str,
    ) -> Result<Entry, CloudError> {
        let url = format!("{}@{}", self.entry_url(at, key), segment(revision_id));
        self.fetch_entry(at, key, url)
    }

    pub fn list_revisions(
        &self,
        at: &StoreRef,
        key: &str,
        page_token: Option<&str>,
    ) -> Result<Page<Revision>, CloudError> {
        let url = format!("{}:listRevisions", self.entry_url(at, key));
        let body = self.paged_get(&url, LIST_REVISIONS, 100, None, page_token, None)?;
        parse_revisions(&body)
    }

    fn write_entry(
        &self,
        method: Method,
        url: &str,
        scope: &str,
        at: &StoreRef,
        key: &str,
        body: &Value,
    ) -> Result<Entry, CloudError> {
        let response = self.send_json(method, url, Some(body))?;
        parse_entry(answer(url, response, scope)?, &at.scope, key)
    }

    pub fn create_entry(
        &self,
        at: &StoreRef,
        key: &str,
        write: &EntryWrite,
    ) -> Result<Entry, CloudError> {
        let url = format!("{}?id={}", self.entries_url(at), segment(key));
        let body = json!({
            "value": write.value,
            "users": user_paths(&write.users),
            "attributes": write.attributes,
        });
        self.write_entry(Method::Post, &url, CREATE, at, key, &body)
    }

    pub fn update_entry(
        &self,
        at: &StoreRef,
        key: &str,
        write: &EntryWrite,
    ) -> Result<Entry, CloudError> {
        let url = self.entry_url(at, key);
        let mut body = json!({
            "value": write.value,
            "users": user_paths(&write.users),
            "attributes": write.attributes,
        });
        if let Some(etag) = &write.etag {
            body["etag"] = Value::String(etag.clone());
        }
        self.write_entry(Method::Patch, &url, UPDATE, at, key, &body)
    }

    pub fn increment_entry(
        &self,
        at: &StoreRef,
        key: &str,
        amount: i64,
        users: &[u64],
        attributes: &Map<String, Value>,
    ) -> Result<Entry, CloudError> {
        let url = format!("{}:increment", self.entry_url(at, key));
        let body = json!({
            "amount": amount,
            "users": user_paths(users),
            "attributes": attributes,
        });
        self.write_entry(Method::Post, &url, UPDATE, at, key, &body)
    }

    pub fn delete_entry(&self, at: &StoreRef, key: &str) -> Result<(), CloudError> {
        let url = self.entry_url(at, key);
        let response = self.send_json(Method::Delete, &url, None)?;
        answer(&url, response, DELETE).map(|_| ())
    }

    /// Writes revision `revision_id`'s value, users and attributes as a new
    /// revision, guarded by `current_etag`.
    pub fn rollback_entry(
        &self,
        at: &StoreRef,
        key: &str,
        revision_id: &str,
        current_etag: &str,
    ) -> Result<Entry, CloudError> {
        let old = self.get_entry_revision(at, key, revision_id)?;
        let write = EntryWrite {
            value: old.value,
            users: old.users,
            attributes: old.attributes,
            etag: Some(current_etag.to_string()),
        };
        self.update_entry(at, key, &write)
    }
}
