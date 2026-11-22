//! "Make available offline": every asset a place refers to (see
//! `rbx_assets::refs`), and every asset those refer to in turn (a model's
//! meshes and scripts, a font family's faces), downloaded into the shared
//! asset cache and kept there for the place, with a manifest of what was
//! packed and what failed.
//!
//! An offline run reads the same cache it always does first, so a packed
//! place loads with no network. A pack is resumable (what's already in the
//! cache is not asked for again) and cancellable (what was done is kept and
//! listed, marked incomplete).

use std::collections::{BTreeSet, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rbx_assets::{AssetCache, AssetKind, AssetRef, AssetResolver};
use serde_json::{json, Value};

/// Downloads at once; `rbx_cloud` backs off on a 429 by itself, and this
/// stays under the viewer's own six so a run started meanwhile still loads.
const WORKERS: usize = 4;

/// Where a pack stands, as its progress callback hears it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    /// Every asset found so far; grows as models reveal what they hold.
    pub found: usize,
    pub failed: usize,
    pub bytes: u64,
}

/// One packed asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub uri: String,
    pub bytes: u64,
}

/// What a pack holds, kept next to the cache as `<key>.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    /// Unix seconds when the pack finished (or was cancelled).
    pub created: u64,
    /// False when the pack was cancelled before it went through everything.
    pub complete: bool,
    pub assets: Vec<Packed>,
    /// `(uri, why)` for every asset that couldn't be had.
    pub failed: Vec<(String, String)>,
}

impl Manifest {
    pub fn bytes(&self) -> u64 {
        self.assets.iter().map(|a| a.bytes).sum()
    }

    pub fn to_json(&self) -> Vec<u8> {
        let doc = json!({
            "created": self.created,
            "complete": self.complete,
            "bytes": self.bytes(),
            "assets": self.assets.iter().map(|a| json!({"uri": a.uri, "bytes": a.bytes})).collect::<Vec<_>>(),
            "failed": self.failed.iter().map(|(uri, why)| json!({"uri": uri, "error": why})).collect::<Vec<_>>(),
        });
        serde_json::to_vec_pretty(&doc).unwrap_or_default()
    }

    pub fn from_json(bytes: &[u8]) -> Option<Manifest> {
        let doc: Value = serde_json::from_slice(bytes).ok()?;
        let list = |key: &str| doc[key].as_array().cloned().unwrap_or_default();
        Some(Manifest {
            created: doc["created"].as_u64()?,
            complete: doc["complete"].as_bool().unwrap_or(false),
            assets: list("assets")
                .iter()
                .filter_map(|a| {
                    Some(Packed {
                        uri: a["uri"].as_str()?.to_owned(),
                        bytes: a["bytes"].as_u64()?,
                    })
                })
                .collect(),
            failed: list("failed")
                .iter()
                .filter_map(|f| {
                    Some((
                        f["uri"].as_str()?.to_owned(),
                        f["error"].as_str().unwrap_or_default().to_owned(),
                    ))
                })
                .collect(),
        })
    }
}

fn manifest_path(cache: &AssetCache, key: &str) -> PathBuf {
    cache.offline_dir().join(format!("{key}.json"))
}

/// The pack `key` made last, if any.
pub fn load(cache: &AssetCache, key: &str) -> Option<Manifest> {
    Manifest::from_json(&std::fs::read(manifest_path(cache, key)).ok()?)
}

/// Removes the pack `key`: its manifest, and the cached assets no other
/// pack keeps. The bytes freed.
pub fn remove(cache: &AssetCache, key: &str) -> u64 {
    let _ = std::fs::remove_file(manifest_path(cache, key));
    cache.unpin(key)
}

/// What every Play (local) needs whatever the place: the stock `Animate`
/// script's animations.
pub fn engine_seeds() -> BTreeSet<AssetRef> {
    crate::character::animate::animation_ids()
        .map(AssetRef::Id)
        .collect()
}

struct State {
    queue: VecDeque<AssetRef>,
    seen: HashSet<AssetRef>,
    active: usize,
    packed: Vec<(AssetRef, u64)>,
    failed: Vec<(String, String)>,
}

/// Packs `seeds` (and all they lead to) for `key` through `resolver`,
/// whose cache keeps them; `progress` hears every finished asset. Saves
/// and returns the manifest, complete or not.
pub fn build(
    resolver: &AssetResolver,
    key: &str,
    seeds: BTreeSet<AssetRef>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Progress) + Sync),
) -> Manifest {
    progress(Progress {
        found: seeds.len(),
        ..Progress::default()
    });
    let state = Mutex::new(State {
        seen: seeds.iter().cloned().collect(),
        queue: seeds.into_iter().collect(),
        active: 0,
        packed: Vec::new(),
        failed: Vec::new(),
    });
    let wake = Condvar::new();
    let worker = || {
        let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let Some(next) = guard.queue.pop_front() else {
                if guard.active == 0 {
                    break;
                }
                guard = wake.wait(guard).unwrap_or_else(|e| e.into_inner());
                continue;
            };
            guard.active += 1;
            drop(guard);
            let result = resolver
                .resolve(&next)
                .map(|asset| (asset.bytes.len() as u64, nested(&asset.bytes, &asset.kind)));
            guard = state.lock().unwrap_or_else(|e| e.into_inner());
            guard.active -= 1;
            match result {
                Ok((size, inner)) => {
                    guard.packed.push((next, size));
                    for found in inner {
                        if guard.seen.insert(found.clone()) {
                            guard.queue.push_back(found);
                        }
                    }
                }
                Err(err) => guard.failed.push((next.uri(), err.to_string())),
            }
            progress(Progress {
                done: guard.packed.len() + guard.failed.len(),
                found: guard.seen.len(),
                failed: guard.failed.len(),
                bytes: guard.packed.iter().map(|(_, size)| size).sum(),
            });
            wake.notify_all();
        }
        wake.notify_all();
    };
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(worker);
        }
    });
    let state = state.into_inner().unwrap_or_else(|e| e.into_inner());
    let cache = resolver.cache();
    let mut packed = state.packed;
    packed.sort();
    // A file read from a local Roblox install isn't cached (see
    // `rbx_assets::native`): it isn't the pack's to keep.
    let pins: Vec<String> = packed
        .iter()
        .filter(|(r, _)| cache.contains(r))
        .filter_map(|(r, _)| cache.entry_of(r))
        .collect();
    let mut failed = state.failed;
    failed.sort();
    let manifest = Manifest {
        created: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        complete: !cancel.load(Ordering::Relaxed),
        assets: packed
            .into_iter()
            .map(|(r, bytes)| Packed {
                uri: r.uri(),
                bytes,
            })
            .collect(),
        failed,
    };
    let _ = cache.pin(key, &pins);
    let _ = std::fs::create_dir_all(cache.offline_dir());
    let _ = std::fs::write(manifest_path(cache, key), manifest.to_json());
    manifest
}

/// The assets an asset's own bytes refer to: a model's (an accessory's
/// mesh, a package's scripts, a decal id's image), a font family's faces.
fn nested(bytes: &[u8], kind: &AssetKind) -> BTreeSet<AssetRef> {
    match kind {
        AssetKind::RobloxBinaryModel | AssetKind::RobloxXmlModel => crate::load::parse_place(bytes)
            .map(|model| rbx_assets::refs::in_dom(&model))
            .unwrap_or_default(),
        _ => serde_json::from_slice::<Value>(bytes)
            .ok()
            .and_then(|doc| doc["faces"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|face| AssetRef::parse(face["assetId"].as_str()?).ok())
            .filter(|r| *r != AssetRef::Empty)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_assets::{FetchError, MemoryFetcher, NativeContent};
    use rbx_dom::{Content, Variant, WeakDom};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    fn temp_cache(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rbx_sim_offline_pack_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn model_with(class: &str, property: &str, uri: &str) -> Vec<u8> {
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        let part = dom.new_instance(class, "Part", Some(model));
        dom.set_property(part, property, Variant::Content(Content::Uri(uri.into())))
            .unwrap();
        rbx_binary::serialize(&dom).unwrap()
    }

    /// Counts what is asked of the network.
    struct Counting(MemoryFetcher, Arc<AtomicUsize>);

    impl rbx_assets::AssetFetcher for Counting {
        fn fetch_id(&self, id: u64) -> Result<Vec<u8>, FetchError> {
            self.1.fetch_add(1, Ordering::Relaxed);
            self.0.fetch_id(id)
        }
    }

    fn resolver(
        dir: &std::path::Path,
        fetcher: impl rbx_assets::AssetFetcher + 'static,
    ) -> AssetResolver {
        let cache = AssetCache::new(Some(dir.to_path_buf())).unwrap();
        let native = NativeContent::new(cache.native_packages_dir());
        AssetResolver::new(cache, Box::new(fetcher), native)
    }

    fn fixture() -> MemoryFetcher {
        let fetcher = MemoryFetcher::new();
        // A package (model) whose mesh is another asset; a font family
        // whose face is another; plain bytes for the leaves.
        fetcher.insert(100, model_with("MeshPart", "MeshId", "rbxassetid://101"));
        fetcher.insert(101, b"version 2.00\nmesh".to_vec());
        fetcher.insert(
            200,
            br#"{"name":"Mine","faces":[{"name":"Regular","weight":400,"style":"normal","assetId":"rbxassetid://201"}]}"#.to_vec(),
        );
        fetcher.insert(201, b"ttf".to_vec());
        fetcher.insert(300, b"OggS sound".to_vec());
        fetcher
    }

    #[test]
    fn a_pack_follows_models_and_fonts_and_lists_what_failed() {
        let dir = temp_cache("follows");
        let resolver = resolver(&dir, fixture());
        let seeds = [100, 200, 300, 404].map(AssetRef::Id).into();
        let heard = AtomicUsize::new(0);
        let manifest = build(
            &resolver,
            "place-1",
            seeds,
            &AtomicBool::new(false),
            &|_| {
                heard.fetch_add(1, Ordering::Relaxed);
            },
        );

        let uris: Vec<&str> = manifest.assets.iter().map(|a| a.uri.as_str()).collect();
        assert_eq!(
            uris,
            [100, 101, 200, 201, 300].map(|id| format!("rbxassetid://{id}"))
        );
        assert_eq!(manifest.failed.len(), 1);
        assert_eq!(manifest.failed[0].0, "rbxassetid://404");
        assert!(manifest.complete);
        assert_eq!(
            heard.load(Ordering::Relaxed),
            7,
            "the start, then each asset"
        );

        let cache = resolver.cache();
        assert_eq!(load(cache, "place-1"), Some(manifest.clone()));
        assert_eq!(cache.trim(0), 0, "a pack's assets outlive a trim");
        assert!(cache.contains(&AssetRef::Id(101)));
        assert_eq!(remove(cache, "place-1"), manifest.bytes());
        assert!(!cache.contains(&AssetRef::Id(101)));
        assert_eq!(load(cache, "place-1"), None);
    }

    #[test]
    fn a_second_pack_resumes_from_the_cache_and_offline_reads_need_no_network() {
        let dir = temp_cache("resume");
        let asked = Arc::new(AtomicUsize::new(0));
        let seeds: BTreeSet<_> = [100, 200].map(AssetRef::Id).into();
        build(
            &resolver(&dir, Counting(fixture(), asked.clone())),
            "place-2",
            seeds.clone(),
            &AtomicBool::new(false),
            &|_| {},
        );
        assert_eq!(asked.load(Ordering::Relaxed), 4);

        // Offline: a fetcher that has nothing.
        let offline = Arc::new(AtomicUsize::new(0));
        let again = build(
            &resolver(&dir, Counting(MemoryFetcher::new(), offline.clone())),
            "place-2",
            seeds,
            &AtomicBool::new(false),
            &|_| {},
        );
        assert_eq!(offline.load(Ordering::Relaxed), 0);
        assert_eq!(again.assets.len(), 4);
        assert!(again.failed.is_empty());
    }

    #[test]
    fn a_cancelled_pack_keeps_what_it_did_and_says_it_is_incomplete() {
        let dir = temp_cache("cancel");
        let resolver = resolver(&dir, fixture());
        let cancel = AtomicBool::new(false);
        let manifest = build(
            &resolver,
            "place-3",
            [100].map(AssetRef::Id).into(),
            &cancel,
            &|p| {
                if p.done == 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        assert!(!manifest.complete);
        assert_eq!(manifest.assets.len(), 1);
        assert_eq!(
            load(resolver.cache(), "place-3").map(|m| m.complete),
            Some(false)
        );
    }

    #[test]
    fn the_stock_animations_are_always_packed() {
        assert!(engine_seeds().contains(&AssetRef::Id(507766388)));
    }
}
