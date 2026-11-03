//! Where `rbxstudio` and `rbxcloud` keep credentials: the OS keyring by
//! default, or whatever `RBX_NATIVE_KEYRING` points at instead.
//!
//! - unset: the OS keyring (Keychain, Credential Manager, Secret Service).
//! - `none`: nothing is read or written; the store always looks empty.
//! - `file:<path>`: a plain JSON file at `<path>`, `{ url: { account,
//!   secret } }`, so tests and scripted runs can hold a key (or seed one)
//!   without ever reaching the user's real keyring. Plaintext on purpose:
//!   it exists for scratch dirs, never for real use.
//!
//! `RBX_NO_KEYRING` (any non-empty value) is the older spelling of `none`.
//! A value this module does not understand also means `none`, so a typo
//! can never fall through to the real keyring.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ApiKey;

/// Where the editor's Open Cloud key lives in whichever store is in use.
pub const API_KEY_URL: &str = "https://apis.roblox.com/rbx-native";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    System,
    Disabled,
    File(PathBuf),
}

/// The store this process should use, read from the environment.
pub fn backend() -> Backend {
    let native = std::env::var("RBX_NATIVE_KEYRING").ok();
    let legacy = std::env::var("RBX_NO_KEYRING").ok();
    match parse(native.as_deref(), legacy.as_deref()) {
        Ok(backend) => backend,
        Err(value) => {
            eprintln!(
                "RBX_NATIVE_KEYRING={value:?} is not `none` or `file:<path>`; not using any keyring"
            );
            Backend::Disabled
        }
    }
}

fn parse(native: Option<&str>, legacy: Option<&str>) -> Result<Backend, String> {
    match native.filter(|v| !v.is_empty()) {
        Some("none") => Ok(Backend::Disabled),
        Some(value) => match value.strip_prefix("file:") {
            Some(path) if !path.is_empty() => Ok(Backend::File(PathBuf::from(path))),
            _ => Err(value.to_string()),
        },
        None if legacy.is_some_and(|v| !v.is_empty()) => Ok(Backend::Disabled),
        None => Ok(Backend::System),
    }
}

#[derive(Serialize, Deserialize)]
struct Entry {
    account: String,
    secret: String,
}

type Entries = BTreeMap<String, Entry>;

fn load(path: &Path) -> io::Result<Entries> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Entries::new()),
        Err(err) => Err(err),
    }
}

fn store(path: &Path, entries: &Entries) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(entries).map_err(io::Error::other)?,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(tmp, path)
}

/// The `(account, secret)` stored under `url` in the file store.
pub fn file_read(path: &Path, url: &str) -> io::Result<Option<(String, Vec<u8>)>> {
    Ok(load(path)?
        .remove(url)
        .map(|entry| (entry.account, entry.secret.into_bytes())))
}

pub fn file_write(path: &Path, url: &str, account: &str, secret: &[u8]) -> io::Result<()> {
    let secret = String::from_utf8(secret.to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "secret is not UTF-8"))?;
    let mut entries = load(path)?;
    let account = account.to_string();
    entries.insert(url.to_string(), Entry { account, secret });
    store(path, &entries)
}

pub fn file_delete(path: &Path, url: &str) -> io::Result<()> {
    let mut entries = load(path)?;
    if entries.remove(url).is_some() {
        store(path, &entries)?;
    }
    Ok(())
}

/// The key `rbxstudio` saved, from whichever store [`backend`] picks, for
/// processes without GPUI's credentials API (the `rbxcloud` CLI, examples).
pub fn stored_api_key() -> Option<ApiKey> {
    let secret = match backend() {
        Backend::Disabled => return None,
        Backend::File(path) => file_read(&path, API_KEY_URL).ok()??.1,
        Backend::System => system_secret(API_KEY_URL)?,
    };
    let key = std::str::from_utf8(&secret).ok()?.trim();
    (!key.is_empty()).then(|| ApiKey::new(key))
}

#[cfg(target_os = "linux")]
fn system_secret(url: &str) -> Option<Vec<u8>> {
    futures_lite::future::block_on(async {
        let keyring = oo7::Keyring::new().await.ok()?;
        keyring.unlock().await.ok()?;
        let items = keyring.search_items(&[("url", url)]).await.ok()?;
        let item = items.into_iter().next()?;
        item.unlock().await.ok()?;
        Some(item.secret().await.ok()?.to_vec())
    })
}

// ponytail: Linux only; macOS Keychain / Windows Credential Manager when the CLI is used there.
#[cfg(not(target_os = "linux"))]
fn system_secret(_url: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variable_picks_the_store() {
        assert_eq!(parse(None, None), Ok(Backend::System));
        assert_eq!(parse(Some(""), Some("")), Ok(Backend::System));
        assert_eq!(parse(Some("none"), None), Ok(Backend::Disabled));
        assert_eq!(parse(None, Some("1")), Ok(Backend::Disabled));
        assert_eq!(
            parse(Some("file:/tmp/k.json"), Some("1")),
            Ok(Backend::File("/tmp/k.json".into()))
        );
        assert!(parse(Some("file:"), None).is_err());
        assert!(parse(Some("nope"), None).is_err());
    }

    #[test]
    fn the_file_store_round_trips() {
        let dir = std::env::temp_dir().join(format!("rbx_cloud_keyring_{}", std::process::id()));
        let path = dir.join("nested/keys.json");
        assert_eq!(file_read(&path, API_KEY_URL).unwrap(), None);
        file_write(&path, API_KEY_URL, "acct", b"secret-1").unwrap();
        file_write(&path, "https://other", "o", b"x").unwrap();
        file_write(&path, API_KEY_URL, "acct", b"secret-2").unwrap();
        assert_eq!(
            file_read(&path, API_KEY_URL).unwrap(),
            Some(("acct".into(), b"secret-2".to_vec()))
        );
        file_delete(&path, API_KEY_URL).unwrap();
        assert_eq!(file_read(&path, API_KEY_URL).unwrap(), None);
        assert!(file_read(&path, "https://other").unwrap().is_some());
        assert!(file_write(&path, API_KEY_URL, "a", &[0xff]).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
