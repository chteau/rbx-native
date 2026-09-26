//! Roblox's API as `luau-lsp` reads it: a Luau definitions file for the
//! types and Roblox's reference text for the descriptions — the same two
//! files its VS Code extension uses — cached under this project's cache
//! folder.
//!
//! The cache remembers which Studio release it was fetched for and asks for
//! nothing while that is still the current one. When Roblox ships, each file
//! is asked for again with the `ETag` it came with, so a file that did not
//! change is never downloaded twice.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Regenerated upstream from each Studio release's API dump. The `None`
/// security level is what an ordinary place script runs at.
const DEFINITIONS_URL: &str =
    "https://raw.githubusercontent.com/JohnnyMorganz/luau-lsp/main/scripts/globalTypes.None.d.luau";
/// Keyed by the same names as the definitions: what fills a completion's or
/// a hover's description.
const DOCUMENTATION_URL: &str =
    "https://raw.githubusercontent.com/MaximumADHD/Roblox-Client-Tracker/roblox/api-docs/en-us.json";
/// The current Studio release, as a short version string (`version-…`).
const STUDIO_VERSION_URL: &str = "https://setup.rbxcdn.com/versionQTStudio";

const DEFINITIONS: &str = "globalTypes.None.d.luau";
const DOCUMENTATION: &str = "en-us.json";
const STAMP: &str = "studio-version";

/// The documentation is about 7 MB, past `ureq`'s default body limit.
const MAX_BODY: u64 = 64 * 1024 * 1024;

pub(super) struct Files {
    pub(super) definitions: PathBuf,
    /// Descriptions only; types and completion work without it.
    pub(super) documentation: Option<PathBuf>,
}

/// The cached files in `dir`, refreshed first if Roblox has shipped since
/// they were fetched. Blocking.
pub(super) fn files(dir: &Path) -> Result<Files, String> {
    let definitions = dir.join(DEFINITIONS);
    let documentation = dir.join(DOCUMENTATION);
    let stamp = dir.join(STAMP);
    let current = studio_version();
    let cached = fs::read_to_string(&stamp).ok();

    if needs_refresh(current.as_deref(), cached.as_deref(), definitions.exists()) {
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        let types = fetch(DEFINITIONS_URL, &definitions);
        // A failed fetch of the descriptions only costs descriptions.
        let _ = fetch(DOCUMENTATION_URL, &documentation);
        // Stamped only once the types actually moved. Upstream regenerates
        // them some time after a release; until it has, the next start asks
        // again, which costs one `304 Not Modified`.
        if let (Some(current), Ok(Fetched::Changed)) = (&current, &types) {
            let _ = fs::write(&stamp, current);
        }
        if let (Err(error), false) = (&types, definitions.exists()) {
            return Err(format!("could not download Roblox's API types: {error}"));
        }
    }
    Ok(Files {
        definitions,
        documentation: documentation.exists().then_some(documentation),
    })
}

/// Whether to ask the network again: always without a copy, never when
/// Roblox's version could not be read (offline — the copy is what there
/// is), and otherwise only when the release moved since the copy.
fn needs_refresh(current: Option<&str>, cached: Option<&str>, have_copy: bool) -> bool {
    match (have_copy, current) {
        (false, _) => true,
        (true, None) => false,
        (true, Some(current)) => cached.map(str::trim) != Some(current.trim()),
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

fn studio_version() -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    let version = agent
        .get(STUDIO_VERSION_URL)
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let version = version.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

#[derive(Debug, PartialEq, Eq)]
enum Fetched {
    Changed,
    Unchanged,
}

/// Downloads `url` over `path` unless the server says the copy there, by the
/// `ETag` it was saved with, is still current.
fn fetch(url: &str, path: &Path) -> Result<Fetched, String> {
    let tag_path = path.with_extension("etag");
    let mut request = agent().get(url);
    if path.exists() {
        if let Ok(tag) = fs::read_to_string(&tag_path) {
            request = request.header("If-None-Match", tag.trim());
        }
    }
    let mut response = request.call().map_err(|error| error.to_string())?;
    if response.status() == 304 {
        return Ok(Fetched::Unchanged);
    }
    let tag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()
        .map_err(|error| error.to_string())?;

    // Renamed into place so a reader never sees half a file.
    let partial = path.with_extension(format!("{}.partial", std::process::id()));
    fs::write(&partial, body).map_err(|error| error.to_string())?;
    fs::rename(&partial, path).map_err(|error| error.to_string())?;
    match tag {
        Some(tag) => fs::write(&tag_path, tag),
        None => fs::remove_file(&tag_path).or(Ok(())),
    }
    .map_err(|error: std::io::Error| error.to_string())?;
    Ok(Fetched::Changed)
}

#[cfg(test)]
mod tests {
    use super::{fetch, needs_refresh, Fetched, DEFINITIONS_URL};

    #[test]
    fn asks_again_only_without_a_copy_or_after_roblox_ships() {
        assert!(needs_refresh(None, None, false));
        assert!(needs_refresh(Some("version-a"), Some("version-a"), false));
        assert!(
            !needs_refresh(None, Some("version-a"), true),
            "offline keeps the copy"
        );
        assert!(!needs_refresh(Some("version-a"), Some("version-a\n"), true));
        assert!(needs_refresh(Some("version-b"), Some("version-a"), true));
        assert!(needs_refresh(Some("version-b"), None, true));
    }

    /// Needs the network. The second fetch of an unchanged file is a `304`.
    #[test]
    #[ignore]
    fn an_unchanged_file_is_not_downloaded_twice() {
        let dir = std::env::temp_dir().join(format!("rbx-luau-api-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("defs.d.luau");
        assert_eq!(fetch(DEFINITIONS_URL, &path), Ok(Fetched::Changed));
        assert!(path.with_extension("etag").exists());
        assert_eq!(fetch(DEFINITIONS_URL, &path), Ok(Fetched::Unchanged));
        let _ = std::fs::remove_dir_all(dir);
    }
}
