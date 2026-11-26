//! Free model kits: CC0 3D packs that the Toolbox's Free kits source
//! browses and the `import_model` MCP tool reads, imported as a `Model`
//! scaled to studs, pivoted at its base centre and credited.
//!
//! Which providers, and why (each checked on its own pages, 2026-11-23):
//!
//! - **Poly Haven** is browsed live. Every asset is CC0
//!   (polyhaven.com/license) and its public API needs no key and allows any
//!   use, asking only for a User-Agent naming the app and a visible credit
//!   wherever its content is shown (`Public-API/ToS.md` 2.4, 2.5).
//! - **Kenney** is CC0 too, but kenney.nl's robots.txt disallows `/assets/`
//!   to every agent and a download link is signed per browser session, so
//!   nothing here fetches from it: the user downloads a pack's zip in a
//!   browser and adds it ([`add_pack`]).
//! - **Quaternius** now publishes under its own licence (QAL, which forbids
//!   redistributing the assets), not CC0, and serves packs from Google Drive
//!   folders; it is not browsed, and an added zip goes by its own licence file.
//! - **Poly Pizza**'s API refuses any call without a key, so it is not offered.
//!
//! An added zip must carry a licence file saying CC0, or it is refused.
//! Everything downloaded or unpacked lives under `<cache>/kits/`, beside
//! the asset cache (`rbx_assets::cache_root`), so browsing again reads the
//! disk instead of the network.

mod import;
mod local;
mod polyhaven;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::RenderImage;

pub(crate) use import::{import, translate, Prepared};
pub(crate) use local::add_pack;

/// Every request names the app, as Poly Haven's API terms ask, and nothing
/// about the user.
const USER_AGENT: &str = concat!("rbx-native/", env!("CARGO_PKG_VERSION"), " (free kits)");
/// `RBX_STUDIO_KITS_DIR=<dir>` keeps the kit cache somewhere else, for tests
/// and captures that must not touch the real one.
const DIR_VARIABLE: &str = "RBX_STUDIO_KITS_DIR";
pub(crate) const LICENSE: &str = "CC0";
/// The most a single click (or `import_model` call) brings in: each model is
/// its own Open Cloud upload.
pub(crate) const BATCH_LIMIT: usize = 12;

/// Studs per metre: creator-docs (`resources/beyond-the-dark/
/// building-architecture.md`) puts a stud closest to 28 cm, and the importer
/// takes a file's units as studs, so a model authored in metres (glTF's own
/// unit, and Poly Haven's) is brought up by this.
pub(crate) const METRES: f32 = 1.0 / 0.28;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Provider {
    PolyHaven,
    /// A zip the user added.
    Local,
}

impl Provider {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "polyhaven" | "poly haven" | "poly_haven" => Some(Provider::PolyHaven),
            "local" => Some(Provider::Local),
            _ => None,
        }
    }
}

/// Where a card's picture comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Thumb {
    Url(String),
    File(PathBuf),
    None,
}

#[derive(Clone, Debug)]
pub(crate) struct KitModel {
    /// A Poly Haven slug, or `<pack>/<file stem>` for an added pack.
    pub(crate) id: String,
    pub(crate) name: String,
    /// Who made it, as its card credits them: a Poly Haven model's own
    /// artists, an added pack's author.
    pub(crate) author: String,
    pub(crate) thumb: Thumb,
}

#[derive(Clone, Debug)]
pub(crate) struct Pack {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) author: String,
    pub(crate) provider: Provider,
    pub(crate) models: Vec<KitModel>,
}

impl Pack {
    /// The pack's cover: its first model's picture.
    pub(crate) fn thumb(&self) -> &Thumb {
        self.models
            .first()
            .map_or(&Thumb::None, |model| &model.thumb)
    }
}

/// The credit attributes an imported model carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Credit {
    pub(crate) source: String,
    /// Left off when empty: a file imported from disk has no known author.
    pub(crate) author: String,
    /// Only claimed for a kit whose licence was checked.
    pub(crate) license: Option<&'static str>,
}

/// A kit model on disk, ready for the importer.
#[derive(Clone, Debug)]
pub(crate) struct Resolved {
    pub(crate) file: PathBuf,
    pub(crate) name: String,
    pub(crate) credit: Credit,
    /// Studs per unit of the file.
    pub(crate) scale: f32,
}

/// Every pack: Poly Haven's (from the cached catalogue when the network is
/// down) and the added ones. The `String` says why Poly Haven's are missing.
pub(crate) fn packs() -> (Vec<Pack>, Option<String>) {
    let mut packs = local::packs();
    match polyhaven::packs() {
        Ok(remote) => {
            packs.extend(remote);
            (packs, None)
        }
        Err(err) => (packs, Some(err)),
    }
}

/// Downloads (once) what `id` needs and says where it is.
pub(crate) fn resolve(provider: Provider, id: &str) -> Result<Resolved, String> {
    match provider {
        Provider::PolyHaven => polyhaven::resolve(id),
        Provider::Local => local::resolve(id),
    }
}

/// A model file from disk as a kit piece: credited by file name only, its
/// licence unknown. A path through a Kenney pack takes that kit's scale, a
/// glTF metres to studs, and an OBJ or FBX, whose units say nothing,
/// stays as Studio's importer leaves it, one unit to the stud.
pub(crate) fn from_file(path: &Path) -> Resolved {
    let shown = path.to_string_lossy();
    let gltf = matches!(
        rbx_import::Format::of(path),
        Some(rbx_import::Format::Gltf | rbx_import::Format::Glb)
    );
    let scale = if shown.to_ascii_lowercase().contains("kenney") {
        local::scale_for("Kenney", &shown)
    } else if gltf {
        METRES
    } else {
        1.0
    };
    let stem = path
        .file_stem()
        .map_or_else(|| "Model".to_owned(), |s| title(&s.to_string_lossy()));
    Resolved {
        file: path.to_path_buf(),
        name: stem,
        credit: Credit {
            source: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            author: String::new(),
            license: None,
        },
        scale,
    }
}

/// Up to `limit` model ids of `provider` whose id or name holds every word
/// of `query`, for a caller that named one that does not exist.
pub(crate) fn search(provider: Provider, query: &str, limit: usize) -> Vec<String> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let (packs, _) = packs();
    let mut found: Vec<String> = packs
        .iter()
        .filter(|pack| pack.provider == provider)
        .flat_map(|pack| &pack.models)
        .filter(|model| {
            let text = format!("{} {}", model.id, model.name).to_ascii_lowercase();
            words.iter().all(|word| text.contains(word.as_str()))
        })
        .map(|model| model.id.clone())
        .collect();
    found.sort();
    found.dedup();
    found.truncate(limit);
    found
}

/// A card's picture, from disk, or downloaded once into the cache.
pub(crate) fn thumbnail(thumb: &Thumb) -> Option<Arc<RenderImage>> {
    let bytes = match thumb {
        Thumb::None => return None,
        Thumb::File(path) => std::fs::read(path).ok()?,
        Thumb::Url(url) => {
            let cached = root()
                .ok()?
                .join("thumbs")
                .join(format!("{:016x}", crate::recovery::fnv1a(url.as_bytes())));
            match std::fs::read(&cached) {
                Ok(bytes) => bytes,
                Err(_) => {
                    let bytes = get(url, 4 << 20).ok()?;
                    write(&cached, &bytes).ok()?;
                    bytes
                }
            }
        }
    };
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = rgba.dimensions();
    crate::render_image::to_render_image(rgba.into_raw(), w, h)
}

#[cfg(test)]
thread_local! {
    /// A test's own kit cache.
    static TEST_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn root() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(root) = TEST_ROOT.with(|root| root.borrow().clone()) {
        return Ok(root);
    }
    std::env::var_os(DIR_VARIABLE)
        .map(PathBuf::from)
        .or_else(|| rbx_assets::cache_root().map(|root| root.join("kits")))
        .ok_or_else(|| "no cache folder to keep kits in (HOME is not set)".to_owned())
}

/// Through the offline gate: offline, a download fails at once with the
/// reason instead of waiting out the timeout.
fn agent() -> ureq::Agent {
    rbx_net::config()
        .user_agent(USER_AGENT)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}

/// A body of at most `limit` bytes.
fn get(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    agent()
        .get(url)
        .call()
        .map_err(|err| format!("{url}: {err}"))?
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|err| format!("{url}: {err}"))
}

/// Writes through a temporary file, so an interrupted download never
/// leaves a half file the cache would take as whole.
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let failed = |err: std::io::Error| format!("{}: {err}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let partial = path.with_extension("partial");
    std::fs::write(&partial, bytes).map_err(failed)?;
    std::fs::rename(&partial, path).map_err(failed)
}

/// `"platformer-kit"` and `"tree_pine-tall"` as `"Platformer Kit"` and
/// `"Tree Pine Tall"`.
fn title(stem: &str) -> String {
    stem.split(['-', '_', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect::<String>()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}
