//! The Open Cloud API key at rest: the OS credential store — the macOS
//! Keychain, Windows' Credential Manager, or the Secret Service (GNOME
//! Keyring, KWallet) on Linux — through GPUI's own credentials API, never a
//! plaintext file.
//!
//! What that buys, honestly: the key is encrypted on disk under the user's
//! login, so a copied disk, a backup, a synced dotfile folder or another
//! user account cannot read it. What it does not: a program already
//! running as this same user in an unlocked session can generally ask the
//! store for it (the Secret Service has no per-app access control; the
//! Keychain prompts). The wizard's IP restriction and expiration steps are
//! what make a stolen key useless elsewhere.
//!
//! The key found here is handed to `rbx_cloud::ApiKey::install`, so every
//! `Client` built afterwards — the viewer's asset fetches included — uses
//! it. `RBX_API_KEY` still wins over it, for scripted launches.

pub(crate) mod accounts;

use gpui_kit::AsyncApp;
use rbx_cloud::ApiKey;

/// The store's lookup key: one entry for this editor.
const URL: &str = "https://apis.roblox.com/rbx-native";
const ACCOUNT: &str = "open-cloud-api-key";

/// Loads the stored key into `rbx_cloud` at startup, moving a key still in
/// the old plaintext `api_key` file into the store first. Returns whether
/// any key is available now — `false` sends a launch to the setup wizard.
pub(crate) async fn restore(cx: &mut AsyncApp) -> bool {
    match read(cx).await {
        Ok(Some(key)) => {
            let known = accounts()
                .find(&accounts::fingerprint(key.expose_secret()))
                .is_some();
            if !known {
                record_active(key.expose_secret(), cx).await;
            }
            ApiKey::install(Some(key));
        }
        Ok(None) => migrate_plaintext(cx).await,
        Err(err) => {
            eprintln!("rbxstudio: the OS credential store could not be read: {err}");
            // Still usable this session, just not moved anywhere safer.
            ApiKey::install(ApiKey::from_plaintext_file());
        }
    }
    ApiKey::from_env_or_config().is_some()
}

/// Stores `key`, replacing any key already there, and makes it the one
/// every later `Client` uses. The wizard calls this only once
/// `rbx_cloud::check_scopes` has passed on it.
pub(crate) async fn save(key: ApiKey, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let secret = key.expose_secret().as_bytes().to_vec();
    cx.update(|cx| cx.write_credentials(URL, ACCOUNT, &secret))
        .await?;
    record_active(key.expose_secret(), cx).await;
    ApiKey::install(Some(key));
    Ok(())
}

/// Removes the stored key and the active account's slot; the next launch
/// opens the wizard again.
pub(crate) async fn forget(cx: &mut AsyncApp) -> anyhow::Result<()> {
    cx.update(|cx| cx.delete_credentials(URL)).await?;
    let mut index = accounts();
    if let Some(id) = index.active {
        if let Err(err) = cx
            .update(|cx| cx.delete_credentials(&accounts::slot_url(id)))
            .await
        {
            eprintln!("rbxstudio: the account's keychain entry could not be deleted: {err}");
        }
        index.remove(id);
        save_index(&index);
    }
    ApiKey::install(None);
    Ok(())
}

fn index_path() -> Option<std::path::PathBuf> {
    crate::settings::default_config_dir().map(|dir| dir.join("accounts.json"))
}

fn save_index(index: &accounts::Accounts) {
    let Some(path) = index_path() else {
        return;
    };
    if let Err(err) = index.save(&path) {
        eprintln!("rbxstudio: the accounts index could not be saved: {err}");
    }
}

/// The index of stored accounts; empty when none exists or the config
/// directory is unknown.
pub(crate) fn accounts() -> accounts::Accounts {
    index_path()
        .map(|path| accounts::Accounts::load(&path))
        .unwrap_or_default()
}

/// Names the active account after a successful key check, writing the
/// index only when something changed.
pub(crate) fn label_active(owner: &str, key_name: &str) {
    let before = accounts();
    let mut index = before.clone();
    if index.label_active(owner, key_name) && index != before {
        save_index(&index);
    }
}

/// Makes `secret` the active account, adding it (and its credential-store
/// slot) when new. Failures are logged: the main key must keep working.
async fn record_active(secret: &str, cx: &mut AsyncApp) {
    let mut index = accounts();
    let (id, is_new) = index.add_active(&accounts::fingerprint(secret));
    if is_new {
        let bytes = secret.as_bytes().to_vec();
        let url = accounts::slot_url(id);
        if let Err(err) = cx
            .update(|cx| cx.write_credentials(&url, ACCOUNT, &bytes))
            .await
        {
            eprintln!("rbxstudio: the account's key could not be stored: {err}");
            return;
        }
    }
    save_index(&index);
}

/// Makes a stored account's key the one in use.
pub(crate) async fn switch(id: u32, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let url = accounts::slot_url(id);
    let stored = cx.update(|cx| cx.read_credentials(&url)).await?;
    let secret = stored
        .and_then(|(_, secret)| String::from_utf8(secret).ok())
        .map(|secret| secret.trim().to_string())
        .filter(|secret| !secret.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("That account's key is no longer in the system keychain.")
        })?;
    save(ApiKey::new(secret), cx).await?;
    let mut index = accounts();
    if index.set_active(id) {
        save_index(&index);
    }
    Ok(())
}

/// Deletes an account's stored key; removing the active one also forgets
/// the main entry.
pub(crate) async fn remove(id: u32, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let url = accounts::slot_url(id);
    cx.update(|cx| cx.delete_credentials(&url)).await?;
    let mut index = accounts();
    let was_active = index.active == Some(id);
    index.remove(id);
    save_index(&index);
    if was_active {
        forget(cx).await?;
    }
    Ok(())
}

async fn read(cx: &mut AsyncApp) -> anyhow::Result<Option<ApiKey>> {
    let stored = cx.update(|cx| cx.read_credentials(URL)).await?;
    Ok(stored
        .and_then(|(_, secret)| String::from_utf8(secret).ok())
        .map(|secret| secret.trim().to_string())
        .filter(|secret| !secret.is_empty())
        .map(ApiKey::new))
}

/// Moves a plaintext `api_key` file into the store, deleting the file only
/// once the store reads the same key back. Anything short of that leaves
/// the file in place and uses it as before.
async fn migrate_plaintext(cx: &mut AsyncApp) {
    let Some(key) = ApiKey::from_plaintext_file() else {
        return;
    };
    let stored = save(key.clone(), cx).await.is_ok()
        && read(cx)
            .await
            .ok()
            .flatten()
            .is_some_and(|back| back.expose_secret() == key.expose_secret());
    let Some(path) = ApiKey::plaintext_file_path().filter(|_| stored) else {
        ApiKey::install(Some(key));
        return;
    };
    match std::fs::remove_file(&path) {
        Ok(()) => eprintln!(
            "rbxstudio: moved the API key from {} into the OS credential store",
            path.display()
        ),
        Err(err) => eprintln!(
            "rbxstudio: the API key is now in the OS credential store, but {} \
             could not be deleted: {err}",
            path.display()
        ),
    }
}

/// The Roblox web session from the code sign-in (`shell::roblox_login`),
/// kept beside the API key under its own entry. It is a full account
/// credential, so it only ever lives in the store and in memory.
const SESSION_URL: &str = "https://www.roblox.com/rbx-native";
const SESSION_ACCOUNT: &str = "roblosecurity";

pub(crate) async fn save_session(
    cookie: &rbx_cloud::heartbeats::SessionCookie,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    let secret = cookie.expose_secret().as_bytes().to_vec();
    cx.update(|cx| cx.write_credentials(SESSION_URL, SESSION_ACCOUNT, &secret))
        .await
}

pub(crate) async fn read_session(
    cx: &mut AsyncApp,
) -> anyhow::Result<Option<rbx_cloud::heartbeats::SessionCookie>> {
    let stored = cx.update(|cx| cx.read_credentials(SESSION_URL)).await?;
    Ok(stored
        .and_then(|(_, secret)| String::from_utf8(secret).ok())
        .filter(|secret| !secret.trim().is_empty())
        .map(|secret| rbx_cloud::heartbeats::SessionCookie::new(&secret)))
}

pub(crate) async fn forget_session(cx: &mut AsyncApp) -> anyhow::Result<()> {
    cx.update(|cx| cx.delete_credentials(SESSION_URL)).await
}
