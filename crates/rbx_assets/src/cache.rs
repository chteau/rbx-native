//! On-disk cache for downloaded assets and extracted native content.

use std::path::{Path, PathBuf};

use crate::error::CacheError;

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
        let bytes = std::fs::read(&path).ok()?;
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

    /// Deletes the least recently used downloads until `ids/` and `native/`
    /// together hold at most `limit` bytes, and returns how many bytes it
    /// freed. A read marks an entry used (see [`touch`]), so what a place
    /// still opens with stays. Content packages (`native-packages/`) are
    /// left alone: they are few, and re-fetching one is slow.
    pub fn trim(&self, limit: u64) -> u64 {
        let mut entries: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
        for dir in ["ids", "native"] {
            collect_files(&self.root.join(dir), &mut entries);
        }
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
    // One temp file per write: two threads rewriting the same entry must
    // not share one, or a reader can see it half-written.
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_path = parent.join(format!("{file_name}.tmp-{}-{n}", std::process::id()));

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
