//! On-disk cache for downloaded assets and extracted native content.

use std::path::{Path, PathBuf};

use crate::error::CacheError;
use crate::AssetRef;

/// A directory-backed cache keyed by numeric asset id (`ids/<id>`) or by
/// native content path (`native/<relative path>`).
pub struct AssetCache {
    root: PathBuf,
}

impl AssetCache {
    /// Opens (creating if needed) the cache at `dir`, or the default
    /// `$XDG_CACHE_HOME/rbx-native/assets/` (falling back to
    /// `~/.cache/rbx-native/assets/`, or `%LOCALAPPDATA%\rbx-native\assets\`
    /// on Windows) when `dir` is `None`.
    pub fn new(dir: Option<PathBuf>) -> Result<Self, CacheError> {
        let root = match dir {
            Some(root) => root,
            None => default_cache_dir()?,
        };
        std::fs::create_dir_all(&root).map_err(|source| CacheError::CreateDir {
            path: root.clone(),
            source,
        })?;
        Ok(Self { root })
    }

    /// Where [`crate::NativeContent`] should cache downloaded Studio content
    /// packages, kept alongside (not inside) `ids/`/`native/` so a package zip
    /// is never mistaken for a resolved asset.
    pub fn native_packages_dir(&self) -> PathBuf {
        self.root.join("native-packages")
    }

    pub fn get_id(&self, id: u64) -> Option<Vec<u8>> {
        let path = self.id_path(id);
        // No asset is empty: an empty entry is a write that never finished,
        // fetched again rather than failing every load.
        let bytes = std::fs::read(&path).ok().filter(|b| !b.is_empty())?;
        touch(&path);
        Some(bytes)
    }

    pub fn put_id(&self, id: u64, bytes: &[u8]) -> Result<(), CacheError> {
        write_atomic(&self.id_path(id), bytes)
    }

    pub fn get_native(&self, relative_path: &str) -> Option<Vec<u8>> {
        let path = self.native_path(relative_path);
        let bytes = std::fs::read(&path).ok()?;
        touch(&path);
        Some(bytes)
    }

    /// A thumbnail's bytes as last fetched, for when the web can't answer.
    pub fn get_thumb(&self, spec: &str) -> Option<Vec<u8>> {
        std::fs::read(self.thumb_path(spec)).ok()
    }

    pub fn put_thumb(&self, spec: &str, bytes: &[u8]) -> Result<(), CacheError> {
        write_atomic(&self.thumb_path(spec), bytes)
    }

    /// Where `reference`'s bytes sit, relative to the cache root; `None`
    /// for the empty reference.
    pub fn entry_of(&self, reference: &AssetRef) -> Option<String> {
        Some(match reference {
            AssetRef::Id(id) => format!("ids/{id}"),
            AssetRef::Native(path) => format!("native/{path}"),
            AssetRef::Thumb(spec) => format!("thumbs/{}", thumb_file(spec)),
            AssetRef::Empty => return None,
        })
    }

    /// Whether `reference`'s bytes are on disk.
    pub fn contains(&self, reference: &AssetRef) -> bool {
        self.entry_of(reference)
            .is_some_and(|entry| self.root.join(entry).is_file())
    }

    /// Offline packs: their manifests, and the entries each one keeps
    /// out of [`trim`](Self::trim)'s reach.
    pub fn offline_dir(&self) -> PathBuf {
        self.root.join("offline")
    }

    /// Keeps `entries` (as [`entry_of`](Self::entry_of) names them) for
    /// the pack `key`, replacing what it kept before.
    pub fn pin(&self, key: &str, entries: &[String]) -> Result<(), CacheError> {
        write_atomic(&self.pins_path(key), entries.join("\n").as_bytes())
    }

    /// Every entry some pack keeps.
    fn pinned(&self) -> std::collections::HashSet<PathBuf> {
        let Ok(files) = std::fs::read_dir(self.offline_dir()) else {
            return Default::default();
        };
        files
            .flatten()
            .filter(|f| f.path().extension().is_some_and(|e| e == "pins"))
            .filter_map(|f| std::fs::read_to_string(f.path()).ok())
            .flat_map(|text| {
                text.lines()
                    .map(|line| self.root.join(line))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Drops the pack `key`: its pins, and the entries no other pack
    /// keeps. The bytes freed.
    pub fn unpin(&self, key: &str) -> u64 {
        let path = self.pins_path(key);
        let entries = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        let kept = self.pinned();
        let mut freed = 0;
        for entry in entries.lines().map(|line| self.root.join(line)) {
            if kept.contains(&entry) {
                continue;
            }
            let size = std::fs::metadata(&entry).map_or(0, |m| m.len());
            if std::fs::remove_file(&entry).is_ok() {
                freed += size;
            }
        }
        freed
    }

    fn pins_path(&self, key: &str) -> PathBuf {
        self.offline_dir().join(format!("{key}.pins"))
    }

    fn thumb_path(&self, spec: &str) -> PathBuf {
        self.root.join("thumbs").join(thumb_file(spec))
    }

    /// Least recently used first, until the cache is under `limit` bytes;
    /// what an offline pack keeps stays. The bytes freed.
    pub fn trim(&self, limit: u64) -> u64 {
        let mut entries: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
        for dir in ["ids", "native", "thumbs"] {
            collect_files(&self.root.join(dir), &mut entries);
        }
        let pinned = self.pinned();
        entries.retain(|(_, _, path)| !pinned.contains(path));
        let mut total: u64 = entries.iter().map(|(_, size, _)| size).sum();
        if total <= limit {
            return 0;
        }
        entries.sort();
        let mut freed = 0;
        for (_, size, path) in entries {
            if total <= limit {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
                freed += size;
            }
        }
        freed
    }

    /// [`Self::trim`] to [`DEFAULT_LIMIT`], or to `RBX_NATIVE_ASSET_CACHE_MB`
    /// megabytes when that is set.
    pub fn trim_to_default(&self) -> u64 {
        let limit = non_empty_env("RBX_NATIVE_ASSET_CACHE_MB")
            .and_then(|mb| mb.parse::<u64>().ok())
            .map_or(DEFAULT_LIMIT, |mb| mb * 1024 * 1024);
        self.trim(limit)
    }

    pub fn put_native(&self, relative_path: &str, bytes: &[u8]) -> Result<(), CacheError> {
        write_atomic(&self.native_path(relative_path), bytes)
    }

    fn id_path(&self, id: u64) -> PathBuf {
        self.root.join("ids").join(id.to_string())
    }

    fn native_path(&self, relative_path: &str) -> PathBuf {
        self.root.join("native").join(relative_path)
    }
}

/// How much the downloaded-asset cache may hold before the least recently
/// used entries go: 32 GiB, roomy enough that an existing cache isn't cut
/// down the first time this runs. `RBX_NATIVE_ASSET_CACHE_MB` sets a tighter one.
pub const DEFAULT_LIMIT: u64 = 32 * 1024 * 1024 * 1024;

/// Marks a cache entry as just used, by its modified time, which is what
/// [`AssetCache::trim`] evicts by. Best effort: a read-only cache still reads.
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
}

/// Every file under `dir`, with its modified time and size.
fn collect_files(dir: &Path, into: &mut Vec<(std::time::SystemTime, u64, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_files(&entry.path(), into);
        } else if let Ok(modified) = meta.modified() {
            into.push((modified, meta.len(), entry.path()));
        }
    }
}

fn default_cache_dir() -> Result<PathBuf, CacheError> {
    cache_root()
        .map(|root| root.join("assets"))
        .ok_or(CacheError::NoCacheDir)
}

/// This project's per-user cache folder, `$XDG_CACHE_HOME/rbx-native`,
/// `%LOCALAPPDATA%\rbx-native`, or `~/.cache/rbx-native`: which the asset
/// cache and anything else disposable live under.
pub fn cache_root() -> Option<PathBuf> {
    if let Some(xdg) = non_empty_env("XDG_CACHE_HOME") {
        return Some(PathBuf::from(xdg).join("rbx-native"));
    }
    // Windows has no XDG/HOME convention of its own; %LOCALAPPDATA% is its
    // non-roaming per-user data dir, the natural match for a cache. Checked
    // before HOME so a Windows-native launch never depends on HOME being set
    // (it usually isn't, outside Git Bash/WSL).
    if let Some(local) = non_empty_env("LOCALAPPDATA") {
        return Some(PathBuf::from(local).join("rbx-native"));
    }
    let home = non_empty_env("HOME")?;
    Some(PathBuf::from(home).join(".cache").join("rbx-native"))
}

/// A thumbnail spec (`type=Asset&id=1&w=150&h=150`) as one file name.
fn thumb_file(spec: &str) -> String {
    spec.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "=&-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Writes via a temp file + rename so a reader never observes a partially
/// written cache entry, and a crash mid-write can't corrupt an existing one.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| CacheError::CreateDir {
        path: parent.to_path_buf(),
        source,
    })?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = parent.join(format!("{file_name}.tmp-{}", std::process::id()));

    std::fs::write(&tmp_path, bytes).map_err(|source| CacheError::Write {
        path: tmp_path.clone(),
        source,
    })?;
    std::fs::rename(&tmp_path, path).map_err(|source| CacheError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    /// Guards every test below that mutates `XDG_CACHE_HOME`/`LOCALAPPDATA`:
    /// env vars are process-global, and `cargo test` runs tests on separate
    /// threads of the same process, so two such tests running concurrently
    /// would each observe the other's value.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("rbx_assets_cache_test_{}_{n}", std::process::id()))
    }

    #[test]
    fn trim_drops_the_least_recently_used_entries_first() {
        let cache = AssetCache::new(Some(temp_dir())).unwrap();
        for id in 1..=4 {
            cache.put_id(id, &[0u8; 100]).unwrap();
        }
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for id in 1..=4 {
            let file = std::fs::File::options()
                .write(true)
                .open(cache.id_path(id))
                .unwrap();
            file.set_modified(old + std::time::Duration::from_secs(id))
                .unwrap();
        }
        // Reading 1 makes it the newest, so 2 and 3 go first.
        assert!(cache.get_id(1).is_some());

        assert_eq!(cache.trim(250), 200);

        assert!(cache.get_id(1).is_some());
        assert!(cache.get_id(2).is_none());
        assert!(cache.get_id(3).is_none());
        assert!(cache.get_id(4).is_some());
        assert_eq!(cache.trim(250), 0, "already under the limit");
    }

    #[test]
    fn trim_spares_what_a_pack_keeps_and_unpin_frees_only_the_unshared() {
        let cache = AssetCache::new(Some(temp_dir())).unwrap();
        for id in 1..=3 {
            cache.put_id(id, &[0u8; 100]).unwrap();
        }
        let entry = |id| cache.entry_of(&AssetRef::Id(id)).unwrap();
        cache.pin("place-a", &[entry(1), entry(2)]).unwrap();
        cache.pin("place-b", &[entry(2)]).unwrap();

        assert_eq!(cache.trim(0), 100, "only the unpinned entry goes");
        assert!(cache.contains(&AssetRef::Id(1)));
        assert!(!cache.contains(&AssetRef::Id(3)));

        assert_eq!(cache.unpin("place-a"), 100, "2 is still place-b's");
        assert!(!cache.contains(&AssetRef::Id(1)));
        assert!(cache.contains(&AssetRef::Id(2)));
    }

    #[test]
    fn thumbs_round_trip_under_a_safe_file_name() {
        let cache = AssetCache::new(Some(temp_dir())).unwrap();
        let spec = "type=AvatarHeadShot&id=1&w=150&h=150/../x";
        cache.put_thumb(spec, b"png").unwrap();
        assert_eq!(cache.get_thumb(spec), Some(b"png".to_vec()));
        assert!(cache.contains(&AssetRef::Thumb(spec.into())));
        assert!(!cache.entry_of(&AssetRef::Thumb(spec.into())).unwrap()[7..].contains('/'));
    }

    #[test]
    fn round_trips_id_entries() {
        let cache = AssetCache::new(Some(temp_dir())).unwrap();
        assert_eq!(cache.get_id(42), None);
        cache.put_id(42, b"hello").unwrap();
        assert_eq!(cache.get_id(42), Some(b"hello".to_vec()));
    }

    #[test]
    fn round_trips_native_entries_with_nested_paths() {
        let cache = AssetCache::new(Some(temp_dir())).unwrap();
        assert_eq!(cache.get_native("sky/sun.jpg"), None);
        cache.put_native("sky/sun.jpg", b"jpegbytes").unwrap();
        assert_eq!(cache.get_native("sky/sun.jpg"), Some(b"jpegbytes".to_vec()));
    }

    #[test]
    fn overwrites_existing_entries() {
        let cache = AssetCache::new(Some(temp_dir())).unwrap();
        cache.put_id(1, b"first").unwrap();
        cache.put_id(1, b"second").unwrap();
        assert_eq!(cache.get_id(1), Some(b"second".to_vec()));
    }

    #[test]
    fn creates_root_directory_recursively() {
        let dir = temp_dir().join("nested").join("deeper");
        let cache = AssetCache::new(Some(dir.clone())).unwrap();
        assert!(dir.is_dir());
        cache.put_id(7, b"x").unwrap();
    }

    #[test]
    fn default_dir_honors_xdg_cache_home() {
        // SAFETY: env var mutation is process-global; guarded by ENV_LOCK so
        // this can't interleave with the other tests below that also mutate
        // XDG_CACHE_HOME/LOCALAPPDATA/HOME, scope the check to the
        // pure-computation helper instead of asserting on the process-wide
        // environment.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        std::env::set_var("XDG_CACHE_HOME", &dir);
        let result = default_cache_dir();
        std::env::remove_var("XDG_CACHE_HOME");
        assert_eq!(result.unwrap(), dir.join("rbx-native").join("assets"));
    }

    #[test]
    fn default_dir_falls_back_to_localappdata_on_windows() {
        // SAFETY: see default_dir_honors_xdg_cache_home above.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        std::env::remove_var("XDG_CACHE_HOME");
        std::env::set_var("LOCALAPPDATA", &dir);
        let result = default_cache_dir();
        std::env::remove_var("LOCALAPPDATA");
        assert_eq!(result.unwrap(), dir.join("rbx-native").join("assets"));
    }

    #[test]
    fn xdg_cache_home_wins_over_localappdata() {
        // SAFETY: see default_dir_honors_xdg_cache_home above.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let xdg = temp_dir();
        let local = temp_dir();
        std::env::set_var("XDG_CACHE_HOME", &xdg);
        std::env::set_var("LOCALAPPDATA", &local);
        let result = default_cache_dir();
        std::env::remove_var("XDG_CACHE_HOME");
        std::env::remove_var("LOCALAPPDATA");
        assert_eq!(result.unwrap(), xdg.join("rbx-native").join("assets"));
    }
}
