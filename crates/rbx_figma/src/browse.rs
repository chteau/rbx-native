//! What the Figma browser shows with `file_content:read` alone, the only
//! scope a public OAuth app gets: files the user opened before (kept
//! locally, since no endpoint lists them), a file's pages and top-level
//! frames, and small previews of both, cached on disk.
//!
//! Folder listing (`GET /v2/teams/:id/folders`, `/v2/folders/:id/files`)
//! needs `folders:read`, which Figma doesn't grant public OAuth apps, so it
//! isn't here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::api::Session;

/// Frame previews are drawn this small: enough for a thumbnail row.
pub const PREVIEW_SCALE: f32 = 0.25;
/// How many files the recent list keeps.
const RECENT: usize = 30;

/// One file the user opened, newest first in [`Recent`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentFile {
    pub key: String,
    pub name: String,
    pub thumbnail_url: Option<String>,
    /// Seconds since the epoch.
    pub opened: u64,
}

/// The browser's home list, saved as JSON after every change.
#[derive(Debug, Default)]
pub struct Recent {
    path: Option<PathBuf>,
    pub files: Vec<RecentFile>,
}

impl Recent {
    /// The list at `path`, empty if it doesn't exist or doesn't parse.
    pub fn open(path: PathBuf) -> Self {
        let files = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Recent {
            path: Some(path),
            files,
        }
    }

    /// Moves `file` to the top, replacing an older entry for the same key.
    pub fn touch(&mut self, file: RecentFile) {
        self.files.retain(|f| f.key != file.key);
        self.files.insert(0, file);
        self.files.truncate(RECENT);
        self.save();
    }

    pub fn forget(&mut self, key: &str) {
        self.files.retain(|f| f.key != key);
        self.save();
    }

    fn save(&self) {
        if let Some(path) = &self.path {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(
                path,
                serde_json::to_vec_pretty(&self.files).unwrap_or_default(),
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub id: String,
    pub name: String,
    /// Figma's node type: `FRAME`, `COMPONENT`, `SECTION`…
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub id: String,
    pub name: String,
    pub frames: Vec<Frame>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub key: String,
    pub name: String,
    pub thumbnail_url: Option<String>,
    pub pages: Vec<Page>,
}

impl Outline {
    pub fn recent(&self, opened: u64) -> RecentFile {
        RecentFile {
            key: self.key.clone(),
            name: self.name.clone(),
            thumbnail_url: self.thumbnail_url.clone(),
            opened,
        }
    }
}

/// A file's pages and their top-level frames (`GET /v1/files/:key?depth=2`).
pub fn outline(session: &mut Session, key: &str) -> Result<Outline, String> {
    let file = session.get_json(&format!("/v1/files/{key}?depth=2"))?;
    Ok(outline_of(key, &file))
}

fn outline_of(key: &str, file: &Value) -> Outline {
    let str_of = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let children = |v: &Value| {
        v.get("children")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let pages = file
        .get("document")
        .map(children)
        .unwrap_or_default()
        .iter()
        .map(|page| Page {
            id: str_of(page, "id"),
            name: str_of(page, "name"),
            frames: children(page)
                .iter()
                .filter(|f| f.get("visible").and_then(Value::as_bool) != Some(false))
                .map(|f| Frame {
                    id: str_of(f, "id"),
                    name: str_of(f, "name"),
                    kind: str_of(f, "type"),
                })
                .collect(),
        })
        .collect();
    Outline {
        key: key.to_string(),
        name: str_of(file, "name"),
        thumbnail_url: file
            .get("thumbnailUrl")
            .and_then(Value::as_str)
            .map(str::to_string),
        pages,
    }
}

/// Small renders of `ids` in file `key`, as node id to a signed URL.
pub fn preview_urls(
    session: &mut Session,
    key: &str,
    ids: &[String],
) -> Result<BTreeMap<String, String>, String> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let ids: String = url::form_urlencoded::byte_serialize(ids.join(",").as_bytes()).collect();
    let answer = session.get_json(&format!(
        "/v1/images/{key}?ids={ids}&format=png&scale={PREVIEW_SCALE}"
    ))?;
    Ok(answer
        .get("images")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default())
}

/// A picture cached on disk under `dir` by `key` (a file key, or file key
/// plus node id), fetched again once it is older than `max_age`. A failed
/// fetch falls back to a stale copy when there is one.
pub fn cached_image(
    dir: &Path,
    key: &str,
    max_age: Duration,
    fetch: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<PathBuf, String> {
    let hash = Sha256::digest(key.as_bytes());
    let name: String = hash[..12].iter().map(|b| format!("{b:02x}")).collect();
    let path = dir.join(format!("{name}.png"));
    let age = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok());
    if age.is_some_and(|age| age < max_age) {
        return Ok(path);
    }
    match fetch() {
        Ok(bytes) => {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok(path)
        }
        Err(_) if age.is_some() => Ok(path),
        Err(err) => Err(err),
    }
}

/// Whether an API error is Figma refusing the scope rather than the file.
pub fn missing_scope(err: &str) -> bool {
    err.contains("(403)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_reads_as_pages_and_frames() {
        let file = serde_json::json!({
            "name": "Shop",
            "thumbnailUrl": "https://x/t.png",
            "document": { "children": [
                { "id": "0:1", "name": "Page 1", "type": "CANVAS", "children": [
                    { "id": "1:2", "name": "Shop_Frame", "type": "FRAME" },
                    { "id": "1:3", "name": "Old", "type": "FRAME", "visible": false }
                ]},
                { "id": "0:2", "name": "Empty", "type": "CANVAS" }
            ]}
        });
        let outline = outline_of("K", &file);
        assert_eq!(outline.name, "Shop");
        assert_eq!(outline.thumbnail_url.as_deref(), Some("https://x/t.png"));
        assert_eq!(outline.pages.len(), 2);
        assert_eq!(outline.pages[0].frames.len(), 1);
        assert_eq!(outline.pages[0].frames[0].id, "1:2");
        assert!(outline.pages[1].frames.is_empty());
    }

    #[test]
    fn recent_files_move_to_the_top_and_persist() {
        let dir = std::env::temp_dir().join(format!("rbx_figma_recent_{}", std::process::id()));
        let path = dir.join("recent.json");
        let file = |key: &str, opened| RecentFile {
            key: key.into(),
            name: key.into(),
            thumbnail_url: None,
            opened,
        };
        let mut recent = Recent::open(path.clone());
        recent.touch(file("a", 1));
        recent.touch(file("b", 2));
        recent.touch(file("a", 3));
        let again = Recent::open(path);
        assert_eq!(
            again
                .files
                .iter()
                .map(|f| f.key.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(again.files[0].opened, 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_fresh_image_is_not_fetched_again_and_a_stale_one_survives_a_failure() {
        let dir = std::env::temp_dir().join(format!("rbx_figma_thumbs_{}", std::process::id()));
        let hour = Duration::from_secs(3600);
        let first = cached_image(&dir, "k", hour, || Ok(b"png".to_vec())).unwrap();
        let second = cached_image(&dir, "k", hour, || panic!("fetched again")).unwrap();
        assert_eq!(first, second);
        let stale = cached_image(&dir, "k", Duration::ZERO, || Err("offline".into())).unwrap();
        assert_eq!(std::fs::read(stale).unwrap(), b"png");
        assert!(cached_image(&dir, "other", hour, || Err("offline".into())).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
