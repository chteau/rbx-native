//! Installing a theme straight from a GitHub repository link.
//!
//! The repository's archive is downloaded, the folder holding its
//! `manifest.json` is unpacked beside the installed themes, and it is loaded
//! exactly as the editor would load it — manifest, palette, widgets, icons —
//! before it replaces anything. A download that is not a valid theme leaves
//! the themes folder as it was.
//!
//! Nothing calls this yet: it waits for the settings screen that will offer
//! it. Everything here is a stranger's data, so the archive is bounded in
//! size and file count and every path in it is checked before a byte is
//! written.

#![expect(dead_code, reason = "for the settings screen")]

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::pack::{self, Manifest, ThemePack};
use super::DEFAULT_ID;

/// A theme is JSON, SVGs and a few images; a repository archive far past
/// this is not a theme.
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 128 * 1024 * 1024;
const MAX_FILES: usize = 4096;

/// Where a link points: `owner/repo`, and a branch or tag when it named one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source {
    owner: String,
    repo: String,
    reference: Option<String>,
}

impl Source {
    /// `https://github.com/owner/repo`, with or without the scheme, `www.`,
    /// a trailing `.git` or `/`, and optionally `/tree/<branch or tag>`.
    pub(crate) fn parse(link: &str) -> Result<Self, String> {
        let invalid = || format!("{link:?} is not a GitHub repository link");
        let rest = link.trim();
        let rest = rest.split(['?', '#']).next().unwrap_or(rest);
        let rest = rest
            .strip_prefix("https://")
            .or_else(|| rest.strip_prefix("http://"))
            .unwrap_or(rest);
        let rest = rest.strip_prefix("www.").unwrap_or(rest);
        let rest = rest.strip_prefix("github.com/").ok_or_else(invalid)?;
        let mut parts = rest.trim_end_matches('/').splitn(4, '/');
        let owner = parts.next().unwrap_or_default();
        let repo = parts.next().unwrap_or_default();
        let repo = repo.strip_suffix(".git").unwrap_or(repo);
        let reference = match (parts.next(), parts.next()) {
            (None, _) => None,
            (Some("tree"), Some(reference)) if !reference.is_empty() => Some(reference.to_owned()),
            _ => return Err(invalid()),
        };
        let name_ok = |name: &str| {
            !name.is_empty()
                && name != "."
                && name != ".."
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        let reference_ok = reference
            .as_deref()
            .is_none_or(|reference| reference.split('/').all(name_ok));
        if !name_ok(owner) || !name_ok(repo) || !reference_ok {
            return Err(invalid());
        }
        Ok(Source {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            reference,
        })
    }

    fn archive_url(&self) -> String {
        format!(
            "https://github.com/{}/{}/archive/{}.zip",
            self.owner,
            self.repo,
            self.reference.as_deref().unwrap_or("HEAD")
        )
    }

    /// The folder it installs into: the repository's name, unless that is
    /// the one name reserved for the built-in theme.
    fn id(&self) -> String {
        if self.repo.eq_ignore_ascii_case(DEFAULT_ID) {
            format!("{}-{}", self.owner, self.repo)
        } else {
            self.repo.clone()
        }
    }
}

/// Downloads and installs the theme `link` points at, replacing an earlier
/// install of the same repository. Blocking: run it off the UI thread.
pub(crate) fn install(link: &str) -> Result<(String, Manifest), String> {
    let themes = pack::themes_dir().ok_or("there is no config directory")?;
    install_into(&themes, link)
}

fn install_into(themes: &Path, link: &str) -> Result<(String, Manifest), String> {
    let source = Source::parse(link)?;
    let archive = download(&source.archive_url())?;
    let id = source.id();
    let manifest = install_archive(themes, &id, &archive)?;
    Ok((id, manifest))
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", "rbx-native")
        .call()
        .map_err(|err| format!("could not download {url}: {err}"))?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_ARCHIVE_BYTES)
        .read_to_vec()
        .map_err(|err| format!("could not download {url}: {err}"))
}

/// Unpacks the theme in `archive` (a zip) into `<themes>/<id>`.
///
/// The theme is the shallowest folder holding a `manifest.json` — a
/// GitHub archive wraps the repository in a `<repo>-<ref>/` folder, and a
/// hand-made zip may not — and only that folder is unpacked.
fn install_archive(themes: &Path, id: &str, archive: &[u8]) -> Result<Manifest, String> {
    let files = unzip(archive)?;
    let root = files
        .iter()
        .map(|(path, _)| path)
        .filter(|path| path.file_name().is_some_and(|name| name == "manifest.json"))
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .min_by_key(|dir| dir.components().count())
        .ok_or("the repository has no manifest.json, so it is not a theme")?;

    fs::create_dir_all(themes).map_err(|err| format!("{}: {err}", themes.display()))?;
    let staging = themes.join(format!(".{id}.installing-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let result = unpack(&files, &root, &staging)
        .and_then(|()| ThemePack::read(id, &staging).map(|pack| pack.manifest))
        .and_then(|manifest| replace(themes, id, &staging).map(|()| manifest));
    let _ = fs::remove_dir_all(&staging);
    result
}

fn unpack(files: &[(PathBuf, Vec<u8>)], root: &Path, staging: &Path) -> Result<(), String> {
    for (path, bytes) in files {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let target = staging.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        fs::write(&target, bytes).map_err(|err| format!("{}: {err}", target.display()))?;
    }
    Ok(())
}

/// Swaps the checked download in for whatever was installed under `id`,
/// keeping the old one until the new one is in place.
fn replace(themes: &Path, id: &str, staging: &Path) -> Result<(), String> {
    let dir = themes.join(id);
    let old = themes.join(format!(".{id}.replaced-{}", std::process::id()));
    let had_old = dir.exists();
    if had_old {
        let _ = fs::remove_dir_all(&old);
        fs::rename(&dir, &old).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    if let Err(err) = fs::rename(staging, &dir) {
        if had_old {
            let _ = fs::rename(&old, &dir);
        }
        return Err(format!("{}: {err}", dir.display()));
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(())
}

/// Every regular file in the archive, by a path `enclosed_name` has
/// already refused to let out of the archive's own root.
fn unzip(archive: &[u8]) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive))
        .map_err(|err| format!("the download is not a zip archive: {err}"))?;
    if zip.len() > MAX_FILES {
        return Err(format!("the archive has more than {MAX_FILES} files"));
    }
    let mut files = Vec::new();
    let mut unpacked = 0u64;
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|err| err.to_string())?;
        if entry.is_dir() || entry.is_symlink() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let mut bytes = Vec::new();
        // `take` rather than trusting the size the archive declares.
        entry
            .take(MAX_UNPACKED_BYTES - unpacked + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| err.to_string())?;
        unpacked += bytes.len() as u64;
        if unpacked > MAX_UNPACKED_BYTES {
            return Err(format!(
                "the archive unpacks to more than {} MiB",
                MAX_UNPACKED_BYTES / (1024 * 1024)
            ));
        }
        files.push((path, bytes));
    }
    Ok(files)
}

#[cfg(test)]
#[path = "github/tests.rs"]
mod tests;
