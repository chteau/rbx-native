//! The registry backend has no "recently published" route, but every publish
//! is a commit `Publish {scope}/{name}@{version}` on the public index repo
//! (`UpliftGames/wally-index`, confirmed against its live history), so the
//! newest commits are the newest publishes. Each package's card is then
//! filled from [`super::metadata`].

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::metadata::{self, Listing};
use crate::settings::{default_config_dir, write_atomic};

const COMMITS: &str = "https://api.github.com/repos/UpliftGames/wally-index/commits?per_page=60";

/// GitHub allows 60 unauthenticated calls an hour per IP, so the package
/// list is fetched at most once per hour and kept in the config dir.
const MAX_AGE_SECS: u64 = 3600;

/// How many distinct packages the Home page lists.
pub(crate) const LIMIT: usize = 6;

#[derive(Deserialize)]
struct Commit {
    commit: Inner,
}

#[derive(Deserialize)]
struct Inner {
    message: String,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    fetched: u64,
    packages: Vec<(String, String)>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn cache_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("wally_recent.json"))
}

fn is_fresh(fetched: u64, now: u64) -> bool {
    now.saturating_sub(fetched) < MAX_AGE_SECS
}

/// The newest published packages: the cached list while it is under an
/// hour old, otherwise the index's commit log (and the cache is rewritten).
fn packages() -> Result<Vec<(String, String)>, String> {
    let path = cache_path();
    let cached = path
        .as_ref()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Cached>(&bytes).ok());
    if let Some(cached) = cached.filter(|cached| is_fresh(cached.fetched, now())) {
        return Ok(cached.packages);
    }
    let commits = super::agent()
        .get(COMMITS)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "rbxstudio")
        .call()
        .map_err(|err| err.to_string())?
        .body_mut()
        .read_json::<Vec<Commit>>()
        .map_err(|err| err.to_string())?;
    let packages = published(commits.iter().map(|commit| commit.commit.message.as_str()));
    if let (Some(path), Ok(bytes)) = (
        path,
        serde_json::to_vec(&Cached {
            fetched: now(),
            packages: packages.clone(),
        }),
    ) {
        let _ = write_atomic(&path, &bytes);
    }
    Ok(packages)
}

pub(crate) fn fetch() -> Result<Vec<Listing>, String> {
    let mut listings = Vec::new();
    let mut first_error = None;
    for (scope, name) in packages()? {
        match metadata::fetch(&scope, &name) {
            Ok(listing) => listings.push(listing),
            Err(message) => {
                first_error.get_or_insert(message);
            }
        }
        if listings.len() == LIMIT {
            break;
        }
    }
    match (listings.is_empty(), first_error) {
        (true, Some(message)) => Err(message),
        _ => Ok(listings),
    }
}

/// The distinct `(scope, name)` of each `Publish scope/name@version`
/// message, newest first; other commits (owner changes, yanks) are skipped.
fn published<'a>(messages: impl Iterator<Item = &'a str>) -> Vec<(String, String)> {
    let mut seen: Vec<(String, String)> = Vec::new();
    for message in messages {
        let Some(rest) = message
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("Publish "))
        else {
            continue;
        };
        let Some((package, _version)) = rest.split_once('@') else {
            continue;
        };
        let Some((scope, name)) = package.split_once('/') else {
            continue;
        };
        let id = (scope.to_owned(), name.to_owned());
        if !seen.contains(&id) {
            seen.push(id);
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_is_fresh_for_an_hour() {
        assert!(is_fresh(1000, 1000 + 3599));
        assert!(!is_fresh(1000, 1000 + 3600));
        assert!(is_fresh(2000, 1000));
    }

    #[test]
    fn publishes_are_kept_newest_first_without_repeats() {
        let messages = [
            "Publish a/one@1.1.0",
            "Add owner for b/*",
            "Publish b/two@0.2.0",
            "Publish a/one@1.0.0",
            "Publish broken",
        ];
        assert_eq!(
            published(messages.into_iter()),
            [("a".into(), "one".into()), ("b".into(), "two".into())]
        );
    }
}
