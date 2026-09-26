//! `luau-lsp` (JohnnyMorganz/luau-lsp) as the script editor's language
//! server: autocomplete, hover and the diagnostics behind both the editor's
//! squiggles and the Script Analysis dock.
//!
//! The server is an external program — found on `PATH`, or wherever
//! [`BINARY_VARIABLE`] points — and never bundled. It reads the place through
//! a [`Mirror`] folder rather than through the DOM, and Roblox's API types
//! from the same definitions file its VS Code extension downloads, cached
//! under this project's cache folder.

mod client;
pub(crate) mod diagnostics;
mod mirror;
mod wire;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

pub(crate) use client::Client;
pub(crate) use mirror::Mirror;

/// Overrides where the server binary is looked for; otherwise `luau-lsp` on
/// `PATH`.
pub(crate) const BINARY_VARIABLE: &str = "RBX_STUDIO_LUAU_LSP";

/// Roblox's API as a Luau definitions file, regenerated upstream from each
/// Studio release's API dump. The `None` security level is what an ordinary
/// place script runs at.
const DEFINITIONS_URL: &str =
    "https://raw.githubusercontent.com/JohnnyMorganz/luau-lsp/main/scripts/globalTypes.None.d.luau";
/// Roblox ships weekly; a week-old copy misses at most one release's API.
const DEFINITIONS_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Loading the definitions file is most of `initialize`'s cost.
const START_TIMEOUT: Duration = Duration::from_secs(60);
/// Long enough for a whole-place `workspace/diagnostic` on a large place.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// A folder of its own per call: a mirror deletes its folder when dropped,
/// so two places opened one after the other in one process must not share.
pub(crate) fn workspace_root() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("rbx-native-luau-{}-{n}", std::process::id()))
}

/// Starts the server over the mirror at `root` and completes the
/// `initialize` handshake. Blocking — downloads the definitions file on a
/// first run — so it belongs on a background thread.
pub(crate) fn start(root: &Path) -> Result<Client, String> {
    let binary = std::env::var_os(BINARY_VARIABLE).unwrap_or_else(|| "luau-lsp".into());
    let definitions = definitions()?;
    let settings = root.join(".luau-lsp-settings.json");
    fs::write(&settings, SETTINGS).map_err(|error| error.to_string())?;

    let mut command = Command::new(&binary);
    command
        .arg("lsp")
        .arg("--stdio")
        .arg(format!("--settings={}", settings.display()))
        .arg(format!("--definitions=@roblox={}", definitions.display()))
        .current_dir(root);
    let client = Client::spawn(command).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            format!("luau-lsp was not found on PATH (set {BINARY_VARIABLE} to its path)")
        }
        _ => format!("could not start {}: {error}", Path::new(&binary).display()),
    })?;

    let root_uri = uri(root);
    let reply = client.request(
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "workspaceFolders": [{"uri": root_uri, "name": "place"}],
            "capabilities": {
                // Byte columns: the editor addresses its text by byte offset
                // already, and the server offers only UTF-8 or UTF-16.
                "general": {"positionEncodings": ["utf-8"]},
                "textDocument": {
                    "completion": {"completionItem": {"snippetSupport": false}},
                    "hover": {"contentFormat": ["markdown", "plaintext"]},
                    "diagnostic": {},
                },
                // Without this the server never takes the change
                // notifications `shell::luau_lsp` sends when the mirror moves.
                "workspace": {"didChangeWatchedFiles": {"dynamicRegistration": true}},
            },
        }),
    );
    let initialized = wait_for(reply, START_TIMEOUT)?;
    if initialized["capabilities"]["positionEncoding"] != "utf-8" {
        return Err("this luau-lsp is too old to speak UTF-8 positions".into());
    }
    client.notify("initialized", json!({}));

    // The server sets a workspace up lazily, on the first request naming a
    // document inside it, and answers `workspace/diagnostic` with
    // ServerCancelled until then. Any document request will do; its own
    // answer (the sourcemap is not Luau) does not matter.
    let _ = client.request(
        "textDocument/diagnostic",
        json!({"textDocument": {"uri": uri(&root.join(mirror::SOURCEMAP))}}),
    );
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        let ready = wait(client.request("workspace/diagnostic", json!({"previousResultIds": []})));
        match ready {
            Ok(_) => return Ok(client),
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error),
        }
    }
}

/// `sourcemap.autogenerate` off: the sourcemap is written by the mirror, not
/// by a `rojo` the server would otherwise try to run. Workspace diagnostics
/// on is what makes Script Analysis cover scripts no tab has open.
const SETTINGS: &str = r#"{
  "luau-lsp.platform.type": "roblox",
  "luau-lsp.sourcemap.enabled": true,
  "luau-lsp.sourcemap.autogenerate": false,
  "luau-lsp.diagnostics.workspace": true
}"#;

/// The cached definitions file, fetched first if missing or stale. A failed
/// refresh keeps the stale copy: old types beat no types.
fn definitions() -> Result<PathBuf, String> {
    let dir = rbx_assets::cache_root()
        .unwrap_or_else(std::env::temp_dir)
        .join("luau-lsp");
    let path = dir.join("globalTypes.None.d.luau");
    let fresh = fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < DEFINITIONS_MAX_AGE);
    if fresh {
        return Ok(path);
    }
    match download(DEFINITIONS_URL) {
        Ok(body) => {
            fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
            fs::write(&path, body).map_err(|error| error.to_string())?;
            Ok(path)
        }
        Err(_) if path.exists() => Ok(path),
        Err(error) => Err(format!("could not download Roblox's API types: {error}")),
    }
}

fn download(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    agent
        .get(url)
        .call()
        .map_err(|error| error.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())
}

/// A request's reply, or why there is none. Blocking.
pub(crate) fn wait(reply: Receiver<Result<Value, String>>) -> Result<Value, String> {
    wait_for(reply, REQUEST_TIMEOUT)
}

fn wait_for(reply: Receiver<Result<Value, String>>, timeout: Duration) -> Result<Value, String> {
    reply
        .recv_timeout(timeout)
        .map_err(|_| "luau-lsp did not answer".to_owned())?
}

pub(crate) fn uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map(String::from)
        .unwrap_or_else(|()| format!("file://{}", path.display()))
}

pub(crate) fn path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

#[cfg(test)]
mod tests;
