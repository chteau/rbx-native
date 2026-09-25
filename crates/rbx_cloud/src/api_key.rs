//! Loading a Roblox Open Cloud API key: `RBX_API_KEY` first, then the key the
//! editor installed from the OS keyring (see [`ApiKey::install`]), then a
//! plaintext config file (XDG on Linux, `%APPDATA%` on Windows), trimmed
//! either way.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// The key a host process read from somewhere this crate cannot reach
/// itself — `rbxstudio`'s OS keyring entry, which only GPUI's platform layer
/// can open. Consulted by [`ApiKey::from_env_or_config`], so every `Client`
/// built afterwards (the viewer's asset fetches included) sees it.
static INSTALLED: RwLock<Option<ApiKey>> = RwLock::new(None);

/// A Roblox Open Cloud API key.
///
/// `Debug` never prints the value: this type ends up in `Client` fields and
/// occasionally in ad-hoc `dbg!()` calls during development, and the key must
/// never leak into logs.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: impl Into<String>) -> Self {
        ApiKey(value.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The raw value, for handing to secure storage. Deliberately not
    /// `as_str`/`Display`: every call site that sees the secret says so.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// Makes `key` the process-wide key [`from_env_or_config`] returns when
    /// `RBX_API_KEY` is unset; `None` withdraws it (a key removed or
    /// replaced from the editor's settings).
    ///
    /// [`from_env_or_config`]: Self::from_env_or_config
    pub fn install(key: Option<ApiKey>) {
        *INSTALLED.write().unwrap_or_else(|e| e.into_inner()) = key;
    }

    /// Where the plaintext key file lives, whether or not it exists — so the
    /// editor can move a key found there into the OS keyring and delete it.
    pub fn plaintext_file_path() -> Option<PathBuf> {
        config_file_path(
            non_empty_env("XDG_CONFIG_HOME"),
            non_empty_env("APPDATA"),
            non_empty_env("HOME"),
        )
    }

    /// The plaintext key file's key alone, ignoring `RBX_API_KEY` and any
    /// installed key.
    pub fn from_plaintext_file() -> Option<Self> {
        read_key_file(&Self::plaintext_file_path()?)
    }

    /// Reads `RBX_API_KEY`, then the key [`install`](Self::install)ed from
    /// the OS keyring, falling back to `$XDG_CONFIG_HOME/rbx-native/api_key`,
    /// `%APPDATA%\rbx-native\api_key` on Windows, or `~/.config/rbx-native/api_key`
    /// as a last resort, trimmed. Returns `None` rather than an error: callers
    /// treat "no key" as "use the anonymous API surface".
    pub fn from_env_or_config() -> Option<Self> {
        let installed = INSTALLED.read().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(key) = installed.filter(|_| non_empty_env("RBX_API_KEY").is_none()) {
            return Some(key);
        }
        resolve(
            non_empty_env("RBX_API_KEY"),
            non_empty_env("XDG_CONFIG_HOME"),
            non_empty_env("APPDATA"),
            non_empty_env("HOME"),
        )
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

fn resolve(
    env_key: Option<String>,
    xdg_config_home: Option<String>,
    appdata: Option<String>,
    home: Option<String>,
) -> Option<ApiKey> {
    if let Some(trimmed) = env_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(ApiKey::new(trimmed));
    }
    let path = config_file_path(xdg_config_home, appdata, home)?;
    read_key_file(&path)
}

fn config_file_path(
    xdg_config_home: Option<String>,
    appdata: Option<String>,
    home: Option<String>,
) -> Option<PathBuf> {
    if let Some(xdg) = xdg_config_home {
        return Some(PathBuf::from(xdg).join("rbx-native").join("api_key"));
    }
    // %APPDATA% is Windows' roaming per-user data dir, checked before HOME so
    // a Windows-native launch never depends on HOME being set (see the same
    // pattern in rbx_studio::settings for a config file, and rbx_assets::
    // AssetCache's %LOCALAPPDATA% for the cache equivalent).
    if let Some(appdata) = appdata {
        return Some(PathBuf::from(appdata).join("rbx-native").join("api_key"));
    }
    Some(
        PathBuf::from(home?)
            .join(".config")
            .join("rbx-native")
            .join("api_key"),
    )
}

fn read_key_file(path: &Path) -> Option<ApiKey> {
    let contents = std::fs::read_to_string(path).ok()?;
    warn_if_group_or_world_readable(path);
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(ApiKey::new(trimmed))
    }
}

#[cfg(unix)]
fn warn_if_group_or_world_readable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        eprintln!(
            "warning: {} is readable by group/other (mode {mode:o}); recommend `chmod 600`",
            path.display()
        );
    }
}

#[cfg(not(unix))]
fn warn_if_group_or_world_readable(_path: &Path) {}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("rbx_cloud_apikey_test_{}_{n}", std::process::id()))
    }

    #[test]
    fn debug_never_prints_the_value() {
        let key = ApiKey::new("super-secret-value");
        let debug = format!("{key:?}");
        assert!(!debug.contains("super-secret-value"));
        assert_eq!(debug, "ApiKey(<redacted>)");
    }

    #[test]
    fn resolve_prefers_env_var_when_set_and_non_empty() {
        let key = resolve(
            Some("  from-env  ".to_string()),
            Some("/should/not/be/used".to_string()),
            None,
            None,
        );
        assert_eq!(key.unwrap().as_str(), "from-env");
    }

    #[test]
    fn resolve_falls_back_to_config_dir_when_env_is_blank() {
        let dir = temp_dir();
        let config_dir = dir.join("rbx-native");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("api_key"), "  from-file\n").unwrap();

        let key = resolve(
            Some("   ".to_string()),
            Some(dir.to_str().unwrap().to_string()),
            None,
            None,
        );
        assert_eq!(key.unwrap().as_str(), "from-file");
    }

    #[test]
    fn resolve_returns_none_when_nothing_is_configured() {
        let dir = temp_dir();
        let key = resolve(None, Some(dir.to_str().unwrap().to_string()), None, None);
        assert!(key.is_none());
    }

    #[test]
    fn resolve_returns_none_for_an_empty_key_file() {
        let dir = temp_dir();
        let config_dir = dir.join("rbx-native");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("api_key"), "   \n").unwrap();

        let key = resolve(None, Some(dir.to_str().unwrap().to_string()), None, None);
        assert!(key.is_none());
    }

    #[test]
    fn config_file_path_prefers_xdg_over_appdata_and_home() {
        let path = config_file_path(
            Some("/xdg/config".to_string()),
            Some("C:\\Users\\user\\AppData\\Roaming".to_string()),
            Some("/home/user".to_string()),
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("/xdg/config/rbx-native/api_key"));
    }

    #[test]
    fn config_file_path_falls_back_to_appdata_on_windows() {
        let path = config_file_path(
            None,
            Some("C:\\Users\\user\\AppData\\Roaming".to_string()),
            Some("/home/user".to_string()),
        )
        .unwrap();
        assert_eq!(
            path,
            PathBuf::from("C:\\Users\\user\\AppData\\Roaming")
                .join("rbx-native")
                .join("api_key")
        );
    }

    #[test]
    fn config_file_path_falls_back_to_home_dot_config() {
        let path = config_file_path(None, None, Some("/home/user".to_string())).unwrap();
        assert_eq!(path, PathBuf::from("/home/user/.config/rbx-native/api_key"));
    }

    #[test]
    fn config_file_path_is_none_without_xdg_appdata_or_home() {
        assert!(config_file_path(None, None, None).is_none());
    }
}
