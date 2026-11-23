//! Packs the user downloaded and added as a zip: checked for a CC0 licence
//! file, unpacked once under `<cache>/kits/local/<pack>/`, and read back
//! from there. Each model is the best file of its name (GLB, then glTF,
//! OBJ, FBX), with the pack's preview picture of the same name as its
//! thumbnail (Kenney's packs ship one per model in `Previews/`).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{
    root, title, write, Credit, KitModel, Pack, Provider, Resolved, Thumb, LICENSE, METRES,
};

/// The model formats, best first: a GLB is one file, a glTF has side files,
/// an OBJ loses its materials' textures and an FBX is uploaded as is.
const FORMATS: [&str; 4] = ["glb", "gltf", "obj", "fbx"];
const MANIFEST: &str = "pack.json";
const FILES: &str = "files";

/// Kenney's kits are authored on a unit grid rather than in metres. Measured
/// on Kenney's own GLBs: a platformer character stands 1.1 units and a high
/// wall 1.7, so 4.5 makes the character a Roblox character's 5 studs and the
/// wall 7.6, a door's height.
pub(super) const KENNEY_SCALE: f32 = 4.5;
/// Kenney's city kits put a building or a road on one unit tile (a small
/// house is 1 x 0.95 x 1), so a tile becomes 24 studs: a two-lane road.
pub(super) const KENNEY_CITY_SCALE: f32 = 24.0;

/// Unpacks `zip` into the cache as a pack, once its licence file says CC0.
pub(crate) fn add_pack(zip: &Path) -> Result<Pack, String> {
    let stem = zip
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let shown = zip.display();
    let file = std::fs::File::open(zip).map_err(|err| format!("{shown}: {err}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| format!("{shown}: {err}"))?;
    let licence = (0..archive.len())
        .find_map(|index| {
            let mut entry = archive.by_index(index).ok()?;
            let name = entry.enclosed_name()?;
            let file = name.file_name()?.to_string_lossy().to_ascii_lowercase();
            if !(file.starts_with("license") || file.starts_with("licence")) {
                return None;
            }
            let mut text = String::new();
            entry.read_to_string(&mut text).ok()?;
            Some(text)
        })
        .ok_or_else(|| {
            format!("{stem} has no licence file, and only packs released as CC0 can be added")
        })?;
    if !is_cc0(&licence) {
        return Err(format!(
            "{stem}\u{2019}s licence isn\u{2019}t CC0, and only CC0 packs can be added"
        ));
    }
    let (author, source) = origin(&stem, &licence);
    let slug: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let dir = root()?.join("local").join(&slug);
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("{shown}: {err}"))?;
        // `enclosed_name` refuses an absolute path or a `..` component, so
        // nothing in the zip lands outside the pack's own folder.
        let Some(path) = entry.enclosed_name().filter(|_| entry.is_file()) else {
            continue;
        };
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|err| format!("{shown}: {err}"))?;
        write(&dir.join(FILES).join(path), &bytes)?;
    }
    let name = title(stem.strip_prefix("kenney_").unwrap_or(&stem));
    let scale = scale_for(&author, &name);
    let manifest = json!({ "name": name, "author": author, "source": source, "scale": scale });
    write(&dir.join(MANIFEST), manifest.to_string().as_bytes())?;
    read(&dir).ok_or_else(|| format!("{stem} holds no .glb, .gltf, .obj or .fbx model"))
}

pub(super) fn packs() -> Vec<Pack> {
    let Ok(entries) = root()
        .and_then(|root| std::fs::read_dir(root.join("local")).map_err(|err| err.to_string()))
    else {
        return Vec::new();
    };
    let mut packs: Vec<Pack> = entries
        .flatten()
        .filter_map(|entry| read(&entry.path()))
        .collect();
    packs.sort_by(|a, b| a.name.cmp(&b.name));
    packs
}

pub(super) fn resolve(id: &str) -> Result<Resolved, String> {
    let (slug, stem) = id
        .split_once('/')
        .ok_or_else(|| format!("{id} is not <pack>/<model>"))?;
    let dir = root()?.join("local").join(slug);
    let manifest = manifest(&dir).ok_or_else(|| format!("no added pack called {slug}"))?;
    let (_, file) = models(&dir.join(FILES))
        .into_iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(stem))
        .ok_or_else(|| format!("{slug} has no model called {stem}"))?;
    Ok(Resolved {
        file,
        name: title(stem),
        credit: Credit {
            source: text(&manifest, "source"),
            author: text(&manifest, "author"),
            license: Some(LICENSE),
        },
        scale: manifest["scale"].as_f64().map_or(METRES, |s| s as f32),
    })
}

pub(super) fn is_cc0(licence: &str) -> bool {
    let lower = licence.to_ascii_lowercase();
    ["cc0", "creative commons zero", "publicdomain/zero"]
        .iter()
        .any(|mark| lower.contains(mark))
}

/// Who made the pack and where its page is, as far as its name and licence
/// tell: Kenney's zips are named `kenney_<page>.zip`.
pub(super) fn origin(stem: &str, licence: &str) -> (String, String) {
    let lower = licence.to_ascii_lowercase();
    if let Some(page) = stem.strip_prefix("kenney_") {
        ("Kenney".into(), format!("https://kenney.nl/assets/{page}"))
    } else if lower.contains("kenney") {
        ("Kenney".into(), "https://kenney.nl".into())
    } else if lower.contains("quaternius") {
        ("Quaternius".into(), "https://quaternius.com".into())
    } else {
        ("Unknown".into(), format!("{stem}.zip"))
    }
}

/// Kenney's grids by kit, and metres (glTF's unit) for everyone else.
pub(super) fn scale_for(author: &str, name: &str) -> f32 {
    let lower = name.to_ascii_lowercase();
    match author {
        "Kenney" if lower.contains("city") || lower.contains("road") => KENNEY_CITY_SCALE,
        "Kenney" => KENNEY_SCALE,
        _ => METRES,
    }
}

fn manifest(dir: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(dir.join(MANIFEST)).ok()?).ok()
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn read(dir: &Path) -> Option<Pack> {
    let manifest = manifest(dir)?;
    let slug = dir.file_name()?.to_string_lossy().into_owned();
    let files = dir.join(FILES);
    let previews = previews(&files);
    let models: Vec<KitModel> = models(&files)
        .into_keys()
        .map(|stem| KitModel {
            id: format!("{slug}/{stem}"),
            name: title(&stem),
            thumb: previews
                .get(&stem.to_ascii_lowercase())
                .map_or(Thumb::None, |path| Thumb::File(path.clone())),
        })
        .collect();
    (!models.is_empty()).then(|| Pack {
        id: format!("local:{slug}"),
        name: text(&manifest, "name"),
        author: text(&manifest, "author"),
        provider: Provider::Local,
        models,
    })
}

/// Each model's best file, by file stem.
pub(super) fn models(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut best: BTreeMap<String, (usize, PathBuf)> = BTreeMap::new();
    for path in walk(dir) {
        let Some(rank) = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|e| FORMATS.iter().position(|f| e.eq_ignore_ascii_case(f)))
        else {
            continue;
        };
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        if best.get(&stem).is_none_or(|(current, _)| rank < *current) {
            best.insert(stem, (rank, path));
        }
    }
    best.into_iter()
        .map(|(stem, (_, path))| (stem, path))
        .collect()
}

/// Every `.png` outside a textures folder, by lowercase stem.
fn previews(dir: &Path) -> BTreeMap<String, PathBuf> {
    walk(dir)
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
                && !path.components().any(|part| {
                    part.as_os_str()
                        .to_string_lossy()
                        .eq_ignore_ascii_case("textures")
                })
        })
        .filter_map(|path| {
            let stem = path.file_stem()?.to_string_lossy().to_ascii_lowercase();
            Some((stem, path))
        })
        .collect()
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
