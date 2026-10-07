//! A whole import: the frame's JSON, the inferred tree, and every picture it
//! needs uploaded once (an on-disk cache remembers what already went up, by
//! Figma's `imageRef` and by PNG hash) and written into `Image`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rbx_dom::Variant;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::api::Session;
use crate::infer::{self, Image, Node};
use crate::link::Link;

/// Renders at twice the design's size, so icons stay sharp on HiDPI.
const RENDER_SCALE: u32 = 2;

fn query(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// The JSON of the frame `link` names.
pub fn fetch_root(session: &mut Session, link: &Link) -> Result<Value, String> {
    let mut answer = session.get_json(&format!(
        "/v1/files/{}/nodes?ids={}",
        link.file_key,
        query(&link.node_id)
    ))?;
    answer
        .pointer_mut(&format!(
            "/nodes/{}/document",
            link.node_id.replace('~', "~0").replace('/', "~1")
        ))
        .map(Value::take)
        .filter(|v| !v.is_null())
        .ok_or_else(|| format!("That file has no frame {}", link.node_id))
}

/// Asset ids already uploaded, saved as JSON after every new one.
#[derive(Debug, Default)]
pub struct Cache {
    path: Option<PathBuf>,
    ids: BTreeMap<String, u64>,
}

impl Cache {
    /// The cache at `path`, empty if it doesn't exist or doesn't parse.
    pub fn open(path: PathBuf) -> Self {
        let ids = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Cache {
            path: Some(path),
            ids,
        }
    }

    fn remember(&mut self, keys: &[String], id: u64) {
        for key in keys {
            self.ids.insert(key.clone(), id);
        }
        if let Some(path) = &self.path {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            // A lost cache only costs a re-upload next time.
            let _ = std::fs::write(
                path,
                serde_json::to_vec_pretty(&self.ids).unwrap_or_default(),
            );
        }
    }
}

fn hash_key(png: &[u8]) -> String {
    let digest = Sha256::digest(png);
    format!(
        "png:{}",
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}

/// Uploads (or finds in `cache`) every node's picture and points its
/// `Image` at the asset. `download` gets an [`Image`]'s PNG; `upload` sends
/// one up under a name and gives its asset id. Stops at the first failure:
/// what went up before is cached, so trying again is cheap.
pub fn resolve_images(
    root: &mut Node,
    cache: &mut Cache,
    mut download: impl FnMut(&Image) -> Result<Vec<u8>, String>,
    mut upload: impl FnMut(&str, &[u8]) -> Result<u64, String>,
    mut progress: impl FnMut(String),
) -> Result<(), String> {
    let total = count(root);
    let mut done = 0;
    let mut result = Ok(());
    root.walk_mut(&mut |node| {
        if result.is_err() {
            return;
        }
        let Some(image) = node.image.clone() else {
            return;
        };
        done += 1;
        progress(format!("Uploading images ({done}/{total})\u{2026}"));
        let by_ref = match &image {
            Image::Fill(image_ref) => Some(format!("ref:{image_ref}")),
            Image::Render(_) => None,
        };
        let cached = by_ref.as_ref().and_then(|k| cache.ids.get(k).copied());
        let id = match cached {
            Some(id) => Ok(id),
            None => download(&image).and_then(|png| {
                let by_hash = hash_key(&png);
                match cache.ids.get(&by_hash).copied() {
                    Some(id) => Ok(id),
                    None => upload(&node.name, &png),
                }
                .inspect(|&id| {
                    cache.remember(
                        &[Some(by_hash), by_ref.clone()]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>(),
                        id,
                    );
                })
            }),
        };
        match id {
            Ok(id) => {
                node.properties.retain(|(n, _)| *n != "Image");
                node.properties
                    .push(("Image", Variant::String(format!("rbxassetid://{id}"))));
            }
            Err(err) => result = Err(format!("{err} (on {:?})", node.name)),
        }
    });
    result
}

fn count(node: &Node) -> usize {
    usize::from(node.image.is_some()) + node.children.iter().map(count).sum::<usize>()
}

/// Fetches, infers and uploads: the tree to insert.
pub fn import(
    session: &mut Session,
    link: &Link,
    cache: &mut Cache,
    upload: impl FnMut(&str, &[u8]) -> Result<u64, String>,
    mut progress: impl FnMut(String),
) -> Result<Node, String> {
    progress("Reading the frame from Figma\u{2026}".into());
    let root = fetch_root(session, link)?;
    let mut tree = infer::infer(&root)?;

    let mut fills = BTreeSet::new();
    let mut renders = BTreeSet::new();
    tree.walk_mut(&mut |node| match &node.image {
        Some(Image::Fill(image_ref)) if !cache.ids.contains_key(&format!("ref:{image_ref}")) => {
            fills.insert(image_ref.clone());
        }
        Some(Image::Render(id)) => {
            renders.insert(id.clone());
        }
        _ => {}
    });
    let mut urls: BTreeMap<String, String> = BTreeMap::new();
    let mut add = |answer: &Value, pointer: &str| {
        if let Some(map) = answer.pointer(pointer).and_then(Value::as_object) {
            urls.extend(
                map.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))),
            );
        }
    };
    if !fills.is_empty() {
        progress("Finding image fills\u{2026}".into());
        add(
            &session.get_json(&format!("/v1/files/{}/images", link.file_key))?,
            "/meta/images",
        );
    }
    if !renders.is_empty() {
        progress(format!("Rendering {} vector pieces\u{2026}", renders.len()));
        let ids = query(&renders.into_iter().collect::<Vec<_>>().join(","));
        add(
            &session.get_json(&format!(
                "/v1/images/{}?ids={ids}&format=png&scale={RENDER_SCALE}",
                link.file_key
            ))?,
            "/images",
        );
    }
    let session = &*session;
    resolve_images(
        &mut tree,
        cache,
        |image| {
            let (Image::Fill(key) | Image::Render(key)) = image;
            let url = urls
                .get(key)
                .ok_or_else(|| format!("Figma gave no picture for {key}; it may be empty"))?;
            session.download(url)
        },
        upload,
        progress,
    )?;
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_node(name: &str, image: Image) -> Node {
        let mut node = infer::infer(&serde_json::json!({ "type": "FRAME", "name": name })).unwrap();
        node.image = Some(image);
        node
    }

    #[test]
    fn each_picture_goes_up_once() {
        let mut root = image_node("Root", Image::Fill("a".into()));
        root.children = vec![
            image_node("Same ref", Image::Fill("a".into())),
            image_node("Same pixels", Image::Render("1:2".into())),
            image_node("New", Image::Render("1:3".into())),
            image_node("Cached", Image::Fill("old".into())),
        ];
        let mut cache = Cache::default();
        cache.ids.insert("ref:old".into(), 7);
        let mut downloads = Vec::new();
        let mut uploads = Vec::new();
        resolve_images(
            &mut root,
            &mut cache,
            |image| {
                downloads.push(image.clone());
                Ok(match image {
                    Image::Render(id) if id == "1:3" => b"other".to_vec(),
                    _ => b"pixels".to_vec(),
                })
            },
            |name, _| {
                uploads.push(name.to_string());
                Ok(100 + uploads.len() as u64)
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(uploads, ["Root", "New"]);
        assert_eq!(
            downloads.len(),
            3,
            "a known imageRef isn't downloaded again"
        );
        let ids: Vec<_> = std::iter::once(&root)
            .chain(&root.children)
            .map(|n| n.get("Image").cloned())
            .collect();
        let id = |n: u64| Some(Variant::String(format!("rbxassetid://{n}")));
        assert_eq!(ids, [id(101), id(101), id(101), id(102), id(7)]);
        assert_eq!(cache.ids.get(&hash_key(b"pixels")), Some(&101));
    }

    #[test]
    fn a_failed_upload_stops_the_import() {
        let mut root = image_node("Root", Image::Render("1:1".into()));
        let err = resolve_images(
            &mut root,
            &mut Cache::default(),
            |_| Ok(vec![1]),
            |_, _| Err("403".into()),
            |_| {},
        )
        .unwrap_err();
        assert!(err.contains("403") && err.contains("Root"));
        assert!(root.get("Image").is_none());
    }
}
