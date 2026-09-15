//! Resolves an [`AssetRef`] to bytes, using the disk cache and falling back
//! to a network fetcher for numeric ids or to [`NativeContent`] for
//! `rbxasset://` paths.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::asset_ref::AssetRef;
use crate::cache::AssetCache;
use crate::error::{AssetError, FetchError};
use crate::native::NativeContent;
use crate::sniff::{sniff, AssetKind};

/// Resolved asset bytes, together with their sniffed format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub bytes: Vec<u8>,
    pub kind: AssetKind,
}

/// The network side of resolving a numeric asset id.
///
/// This crate never talks to the network itself for `rbxassetid://`
/// references — the real implementation lives in the viewer, backed by
/// `rbx_cloud`. Kept as a trait so this crate stays testable offline via
/// [`MemoryFetcher`].
///
/// `Send + Sync` is part of the contract: callers resolve a batch of assets
/// from a handful of threads through a single shared [`AssetResolver`].
pub trait AssetFetcher: Send + Sync {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, FetchError>;
}

/// Combines the disk cache, a network fetcher and native content resolution
/// into a single entry point for turning an [`AssetRef`] into bytes.
pub struct AssetResolver {
    cache: AssetCache,
    fetcher: Box<dyn AssetFetcher>,
    native: NativeContent,
}

impl AssetResolver {
    pub fn new(cache: AssetCache, fetcher: Box<dyn AssetFetcher>, native: NativeContent) -> Self {
        Self {
            cache,
            fetcher,
            native,
        }
    }

    pub fn resolve(&self, r: &AssetRef) -> Result<Asset, AssetError> {
        let bytes = match r {
            AssetRef::Empty => return Err(AssetError::Empty),
            AssetRef::Thumb(_) => return Err(AssetError::Empty),
            AssetRef::Id(id) => self.resolve_id(*id)?,
            AssetRef::Native(path) => self.resolve_native(path)?,
        };
        let kind = sniff(&bytes);
        Ok(Asset { bytes, kind })
    }

    fn resolve_id(&self, id: u64) -> Result<Vec<u8>, AssetError> {
        if let Some(bytes) = self.cache.get_id(id) {
            return Ok(bytes);
        }
        let bytes = self.fetcher.fetch_id(id)?;
        self.cache.put_id(id, &bytes)?;
        Ok(bytes)
    }

    fn resolve_native(&self, path: &str) -> Result<Vec<u8>, AssetError> {
        if let Some(bytes) = self.cache.get_native(path) {
            return Ok(bytes);
        }
        let bytes = self.native.fetch(path)?;
        self.cache.put_native(path, &bytes)?;
        Ok(bytes)
    }
}

/// An in-memory [`AssetFetcher`] for tests: no network, just a lookup table.
#[derive(Default)]
pub struct MemoryFetcher {
    assets: Mutex<HashMap<u64, Vec<u8>>>,
}

impl MemoryFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, id: u64, bytes: Vec<u8>) {
        self.assets.lock().unwrap().insert(id, bytes);
    }
}

impl AssetFetcher for MemoryFetcher {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, FetchError> {
        self.assets
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or(FetchError::NotFound(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::NativeContent;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_cache() -> AssetCache {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rbx_assets_resolver_test_{}_{n}",
            std::process::id()
        ));
        AssetCache::new(Some(dir)).unwrap()
    }

    fn resolver_with(fetcher: MemoryFetcher) -> AssetResolver {
        let cache = temp_cache();
        let native = NativeContent::new(cache.native_packages_dir());
        AssetResolver::new(cache, Box::new(fetcher), native)
    }

    #[test]
    fn resolves_id_via_fetcher_and_sniffs_kind() {
        let fetcher = MemoryFetcher::new();
        fetcher.insert(1, vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        let resolver = resolver_with(fetcher);
        let asset = resolver.resolve(&AssetRef::Id(1)).unwrap();
        assert_eq!(asset.kind, AssetKind::Png);
    }

    #[test]
    fn caches_id_after_first_fetch() {
        let fetcher = MemoryFetcher::new();
        fetcher.insert(1, b"stuff".to_vec());
        let resolver = resolver_with(fetcher);
        resolver.resolve(&AssetRef::Id(1)).unwrap();
        // A second resolve must hit the cache: drop the fetcher's ability to
        // answer by resolving an id it was never told about but that's now cached.
        assert_eq!(resolver.cache.get_id(1), Some(b"stuff".to_vec()));
    }

    #[test]
    fn missing_id_surfaces_fetch_error() {
        let resolver = resolver_with(MemoryFetcher::new());
        let err = resolver.resolve(&AssetRef::Id(999));
        assert!(matches!(
            err,
            Err(AssetError::Fetch(FetchError::NotFound(999)))
        ));
    }

    #[test]
    fn empty_ref_is_an_error() {
        let resolver = resolver_with(MemoryFetcher::new());
        assert!(matches!(
            resolver.resolve(&AssetRef::Empty),
            Err(AssetError::Empty)
        ));
    }
}
