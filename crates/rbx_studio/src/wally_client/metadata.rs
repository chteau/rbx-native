//! `GET /v1/package-metadata/{scope}/{name}` — every published version of
//! one package, each with its manifest (`UpliftGames/wally@f578078:
//! wally-registry-backend/src/main.rs:89-104`, `package_info`). Plain JSON,
//! no auth, confirmed against the live registry: `{"versions": [{"package":
//! {"name", "version", "realm", "description", ...}, "dependencies", ...}]}`.
//! What the dock's Featured cards, a result's default realm and the Updates
//! check read.

use serde::Deserialize;

use super::content::Realm;

#[derive(Debug, Deserialize)]
struct Metadata {
    versions: Vec<VersionEntry>,
}

#[derive(Debug, Deserialize)]
struct VersionEntry {
    package: PackageInfo,
}

#[derive(Debug, Deserialize)]
struct PackageInfo {
    version: String,
    #[serde(default)]
    realm: Realm,
    #[serde(default)]
    description: Option<String>,
}

/// What the dock keeps of one package: its published versions, newest
/// first, and the newest one's realm and description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listing {
    pub(crate) scope: String,
    pub(crate) name: String,
    /// Newest first; never empty.
    pub(crate) versions: Vec<semver::Version>,
    pub(crate) realm: Realm,
    pub(crate) description: Option<String>,
}

impl Listing {
    pub(crate) fn latest(&self) -> &semver::Version {
        &self.versions[0]
    }
}

/// wally.run's home page lists "Popular Packages"; that list is a constant
/// in its frontend, not an API (`UpliftGames/wally@f578078:
/// wally-registry-frontend/src/mocks/popularPackages.mock.js`), mirrored
/// here in its order. Each one's card is filled from [`fetch`].
pub(crate) const FEATURED: [(&str, &str); 6] = [
    ("evaera", "cmdr"),
    ("roblox", "roact"),
    ("evaera", "promise"),
    ("roblox", "testez"),
    ("sleitnick", "knit"),
    ("howmanysmall", "janitor"),
];

pub(crate) fn fetch(scope: &str, name: &str) -> Result<Listing, String> {
    let mut response = super::agent()
        .get(format!("{}/package-metadata/{scope}/{name}", super::base()))
        .call()
        .map_err(|err| err.to_string())?;
    let metadata = response
        .body_mut()
        .read_json::<Metadata>()
        .map_err(|err| err.to_string())?;
    listing_from(scope, name, metadata)
}

/// The pure half of [`fetch`]: versions the registry sent that parse as
/// semver, newest first, and the newest one's manifest fields. A package
/// with no parsable version is an error, not a listing with nothing in it.
fn listing_from(scope: &str, name: &str, metadata: Metadata) -> Result<Listing, String> {
    let mut entries: Vec<(semver::Version, PackageInfo)> = metadata
        .versions
        .into_iter()
        .filter_map(|entry| {
            let version = semver::Version::parse(&entry.package.version).ok()?;
            Some((version, entry.package))
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    let (_, newest) = entries
        .first()
        .ok_or_else(|| format!("{scope}/{name} has no published versions"))?;
    Ok(Listing {
        scope: scope.to_owned(),
        name: name.to_owned(),
        realm: newest.realm,
        description: newest
            .description
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        versions: entries.into_iter().map(|(version, _)| version).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(json: &str) -> Metadata {
        serde_json::from_str(json).expect("the registry's own shape")
    }

    #[test]
    fn decodes_the_real_response_shape() {
        let listing = listing_from("evaera", "promise", metadata(
            r#"{"versions":[{"dependencies":{},"dev-dependencies":{},"package":{"authors":[],"description":"Promise implementation for Roblox","exclude":[],"homepage":null,"include":[],"license":"MIT","name":"evaera/promise","private":false,"realm":"shared","registry":"https://github.com/UpliftGames/wally-index","repository":null,"version":"4.0.0"},"place":{"server-packages":null,"shared-packages":null},"server-dependencies":{}}]}"#,
        ))
        .unwrap();
        assert_eq!(listing.latest().to_string(), "4.0.0");
        assert_eq!(listing.realm, Realm::Shared);
        assert_eq!(
            listing.description.as_deref(),
            Some("Promise implementation for Roblox")
        );
    }

    #[test]
    fn newest_is_by_semver_and_carries_its_own_manifest() {
        let listing = listing_from(
            "s",
            "p",
            metadata(
                r#"{"versions":[
                {"package":{"version":"0.9.0","realm":"shared","description":"old"}},
                {"package":{"version":"0.10.0","realm":"server","description":"new"}},
                {"package":{"version":"0.10.0-rc.1","realm":"server","description":"rc"}},
                {"package":{"version":"not-a-version"}}
            ]}"#,
            ),
        )
        .unwrap();
        let versions: Vec<String> = listing.versions.iter().map(|v| v.to_string()).collect();
        assert_eq!(versions, ["0.10.0", "0.10.0-rc.1", "0.9.0"]);
        assert_eq!(listing.realm, Realm::Server);
        assert_eq!(listing.description.as_deref(), Some("new"));
    }

    #[test]
    fn a_blank_description_is_none_and_no_versions_is_an_error() {
        let listing = listing_from(
            "s",
            "p",
            metadata(r#"{"versions":[{"package":{"version":"1.0.0","description":"  "}}]}"#),
        )
        .unwrap();
        assert!(listing.description.is_none());
        assert!(listing_from("s", "p", metadata(r#"{"versions":[]}"#)).is_err());
    }
}
