//! Poly Haven's models through its public API (`api.polyhaven.com`): the
//! catalogue, refreshed at most daily, grouped into packs by collection and
//! by category, and each model's 1k glTF with its side files, downloaded
//! once.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::{get, root, write, Credit, KitModel, Pack, Provider, Resolved, Thumb, LICENSE, METRES};

const API: &str = "https://api.polyhaven.com";
const AUTHOR: &str = "Poly Haven";
const COLLECTION: &str = "collection: ";
/// How old the cached catalogue may get before it is fetched again.
const FRESH: Duration = Duration::from_secs(24 * 60 * 60);
/// The texture size downloaded: the importer has Roblox resample anything
/// larger down to 1024 anyway.
const RESOLUTION: &str = "1k";

/// Collections first, by name, then categories, largest first.
pub(super) fn packs() -> Result<Vec<Pack>, String> {
    Ok(group(&catalogue()?))
}

pub(super) fn resolve(slug: &str) -> Result<Resolved, String> {
    let catalogue = catalogue()?;
    let asset = catalogue
        .get(slug)
        .ok_or_else(|| format!("Poly Haven has no model called {slug}"))?;
    let file = download(slug)?;
    Ok(Resolved {
        file,
        name: name(slug, asset),
        credit: Credit {
            source: format!("https://polyhaven.com/a/{slug}"),
            author: authors(asset),
            license: Some(LICENSE),
        },
        scale: METRES,
    })
}

/// The catalogue, slug to asset, from the cache while it is fresh, and from
/// a stale cache when the network fails.
fn catalogue() -> Result<BTreeMap<String, Value>, String> {
    let path = root()?.join("polyhaven").join("models.json");
    let age = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    let bytes = match age {
        Some(age) if age < FRESH => std::fs::read(&path).map_err(|err| err.to_string())?,
        _ => match get(&format!("{API}/assets?type=models"), 32 << 20) {
            Ok(bytes) => {
                write(&path, &bytes)?;
                bytes
            }
            Err(err) => std::fs::read(&path)
                .map_err(|_| format!("couldn\u{2019}t reach Poly Haven ({err})"))?,
        },
    };
    serde_json::from_slice(&bytes).map_err(|err| format!("Poly Haven\u{2019}s catalogue: {err}"))
}

pub(super) fn group(catalogue: &BTreeMap<String, Value>) -> Vec<Pack> {
    let mut collections: BTreeMap<String, Vec<KitModel>> = BTreeMap::new();
    let mut categories: BTreeMap<String, Vec<KitModel>> = BTreeMap::new();
    for (slug, asset) in catalogue {
        let model = KitModel {
            id: slug.clone(),
            name: name(slug, asset),
            author: authors(asset),
            thumb: Thumb::Url(format!(
                "https://cdn.polyhaven.com/asset_img/thumbs/{slug}.png?width=256&height=256"
            )),
        };
        let tags: Vec<&str> = asset["categories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for tag in &tags {
            if let Some(collection) = tag.strip_prefix(COLLECTION) {
                collections
                    .entry(collection.to_owned())
                    .or_default()
                    .push(model.clone());
            }
        }
        if let Some(category) = tags.iter().find(|tag| !tag.starts_with(COLLECTION)) {
            categories
                .entry((*category).to_owned())
                .or_default()
                .push(model);
        }
    }
    let pack = |kind: &str, key: String, mut models: Vec<KitModel>| {
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Pack {
            id: format!("polyhaven:{kind}/{key}"),
            name: super::title(&key),
            author: AUTHOR.to_owned(),
            provider: Provider::PolyHaven,
            models,
        }
    };
    let mut by_size: Vec<(String, Vec<KitModel>)> = categories.into_iter().collect();
    by_size.sort_by_key(|(_, models)| std::cmp::Reverse(models.len()));
    collections
        .into_iter()
        .map(|(key, models)| pack("collection", key, models))
        .chain(
            by_size
                .into_iter()
                .map(|(key, models)| pack("category", key, models)),
        )
        .collect()
}

fn name(slug: &str, asset: &Value) -> String {
    asset["name"]
        .as_str()
        .map_or_else(|| super::title(slug), str::to_owned)
}

fn authors(asset: &Value) -> String {
    let names: Vec<&str> = asset["authors"]
        .as_object()
        .map(|authors| authors.keys().map(String::as_str).collect())
        .unwrap_or_default();
    if names.is_empty() {
        AUTHOR.to_owned()
    } else {
        names.join(", ")
    }
}

/// The model's `.gltf` with its buffer and textures beside it, downloaded
/// once.
fn download(slug: &str) -> Result<PathBuf, String> {
    let dir = root()?.join("polyhaven").join("models").join(slug);
    // Names the `.gltf` once every file is in, so a cached model needs no call.
    let complete = dir.join("complete");
    if let Ok(main) = std::fs::read_to_string(&complete) {
        if dir.join(main.trim()).is_file() {
            return Ok(dir.join(main.trim()));
        }
    }
    let files: Value = serde_json::from_slice(&get(&format!("{API}/files/{slug}"), 4 << 20)?)
        .map_err(|err| format!("Poly Haven\u{2019}s files for {slug}: {err}"))?;
    let gltf = &files["gltf"][RESOLUTION]["gltf"];
    let url = gltf["url"]
        .as_str()
        .ok_or_else(|| format!("Poly Haven has no {RESOLUTION} glTF of {slug}"))?;
    let main = url
        .rsplit('/')
        .next()
        .filter(|name| name.ends_with(".gltf"))
        .ok_or_else(|| format!("Poly Haven\u{2019}s glTF link for {slug} is not a .gltf"))?;
    let mut wanted = vec![(PathBuf::from(main), url.to_owned())];
    for (relative, entry) in gltf["include"].as_object().into_iter().flatten() {
        let relative = enclosed(relative)
            .ok_or_else(|| format!("Poly Haven named an unsafe path for {slug}: {relative}"))?;
        let url = entry["url"].as_str().ok_or_else(|| {
            format!(
                "Poly Haven gave no link for {slug}'s {}",
                relative.display()
            )
        })?;
        wanted.push((relative, url.to_owned()));
    }
    for (relative, url) in &wanted {
        let path = dir.join(relative);
        if !path.is_file() {
            write(&path, &get(url, 256 << 20)?)?;
        }
    }
    write(&complete, main.as_bytes())?;
    Ok(dir.join(main))
}

/// `relative` when it stays inside the folder it is joined to.
pub(super) fn enclosed(relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    path.components()
        .all(|part| matches!(part, Component::Normal(_)))
        .then(|| path.to_path_buf())
}
