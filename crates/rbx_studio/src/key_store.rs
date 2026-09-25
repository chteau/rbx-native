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
        Ok(Some(key)) => ApiKey::install(Some(key)),
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
    ApiKey::install(Some(key));
    Ok(())
}

/// Removes the stored key; the next launch opens the wizard again.
pub(crate) async fn forget(cx: &mut AsyncApp) -> anyhow::Result<()> {
    cx.update(|cx| cx.delete_credentials(URL)).await?;
    ApiKey::install(None);
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
