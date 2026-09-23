//! `GET /v1/package-contents/{scope}/{name}/{version}` — a zip of the
//! package's raw source tree, `wally.toml` included. Needs a
//! `Wally-Version` header (confirmed by hand: 426 without it — the
//! registry only checks that *something* plausible is sent, not that this
//! is the real `wally` binary).

use std::collections::BTreeMap;
use std::io::{Cursor, Read as _};

use serde::Deserialize;

use super::tree::{self, PackageNode};

const CLIENT_VERSION: &str = "0.3.2";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Realm {
    #[default]
    Shared,
    Server,
    Dev,
}

impl Realm {
    pub(crate) const ALL: [Realm; 3] = [Realm::Shared, Realm::Server, Realm::Dev];

    /// The manifest section's name, as the dock's realm switch shows it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Realm::Shared => "Shared",
            Realm::Server => "Server",
            Realm::Dev => "Dev",
        }
    }

    /// Whether a package of realm `package` may be listed under the
    /// dependency section `self`: Wally's own rule, `Realm::
    /// is_dependency_valid` (`UpliftGames/wally@f578078:src/manifest.rs:
    /// 178-186`) — a `shared` section only takes `shared` packages, the
    /// `server` and `dev` sections take any.
    pub(crate) fn accepts(self, package: Realm) -> bool {
        matches!(
            (self, package),
            (Realm::Server, _) | (Realm::Dev, _) | (Realm::Shared, Realm::Shared)
        )
    }
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "server-dependencies")]
    server_dependencies: BTreeMap<String, String>,
}

/// One `[dependencies]`/`[server-dependencies]` entry, parsed from its
/// `"scope/name@requirement"` value string — the alias itself (`Comm`,
/// `Promise`, ...) is the table key, kept alongside since it's also the
/// name the installed alias file needs.
pub(crate) struct Dependency {
    pub(crate) alias: String,
    pub(crate) scope: String,
    pub(crate) name: String,
    pub(crate) requirement: semver::VersionReq,
}

pub(crate) struct Manifest {
    pub(crate) dependencies: Vec<Dependency>,
}

pub(crate) struct Package {
    pub(crate) manifest: Manifest,
    pub(crate) tree: PackageNode,
}

/// `"scope/name@requirement"` → its three parts. `None` for anything not
/// shaped like that — an entry this client can't parse is dropped from
/// resolution rather than failing the whole manifest read, the same
/// "degrade instead of refuse" choice `rbx_xml`'s decoders make for an
/// unrecognized property tag.
fn parse_requirement(alias: &str, value: &str) -> Option<Dependency> {
    let (scope, rest) = value.split_once('/')?;
    let (name, req) = rest.split_once('@')?;
    let requirement = semver::VersionReq::parse(req).ok()?;
    Some(Dependency {
        alias: alias.to_owned(),
        scope: scope.to_owned(),
        name: name.to_owned(),
        requirement,
    })
}

/// Deliberately doesn't read `[package].name`/`registry`/`version` from
/// the manifest at all: this client already knows exactly which `scope`/
/// `name`/`version` it asked for (the registry's own search association,
/// not the package's self-report — a `wally.toml` typo or stale rename
/// can't desync this client's identity for it the way trusting the file
/// would), and `resolve::ResolvedPackage` carries that identity
/// separately. Only `realm` and the dependency tables are real inputs.
fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let raw: RawManifest = toml::from_str(text).map_err(|err| err.to_string())?;
    let dependencies = raw
        .dependencies
        .iter()
        .chain(raw.server_dependencies.iter())
        .filter_map(|(alias, value)| parse_requirement(alias, value))
        .collect();
    Ok(Manifest { dependencies })
}

fn unzip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|err| err.to_string())?;
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|err| err.to_string())?;
        if entry.is_dir() {
            continue;
        }
        // `enclosed_name` refuses an absolute path or a `..` component —
        // this zip comes from the network, so a path is trusted only once
        // it's been sanitized against writing outside the tree this client
        // itself builds in memory (there's no filesystem extraction here,
        // but the same discipline costs nothing and holds if that changes).
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|err| err.to_string())?;
        files.push((path.to_string_lossy().replace('\\', "/"), bytes));
    }
    Ok(files)
}

pub(crate) fn fetch(scope: &str, name: &str, version: &semver::Version) -> Result<Package, String> {
    let mut response = super::agent()
        .get(format!(
            "{}/package-contents/{scope}/{name}/{version}",
            super::base()
        ))
        .header("Wally-Version", CLIENT_VERSION)
        .call()
        .map_err(|err| err.to_string())?;
    let bytes = response
        .body_mut()
        .read_to_vec()
        .map_err(|err| err.to_string())?;
    let files = unzip(&bytes)?;
    let manifest_text = files
        .iter()
        .find(|(path, _)| path == "wally.toml")
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
        .ok_or_else(|| "package contents had no wally.toml".to_owned())?;
    let manifest = parse_manifest(&manifest_text)?;
    let tree = tree::build(&files, name);
    Ok(Package { manifest, tree })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_section_takes_only_shared_packages() {
        assert!(Realm::Shared.accepts(Realm::Shared));
        assert!(!Realm::Shared.accepts(Realm::Server));
        assert!(!Realm::Shared.accepts(Realm::Dev));
        assert!(Realm::Server.accepts(Realm::Shared));
        assert!(Realm::Server.accepts(Realm::Server));
        assert!(Realm::Dev.accepts(Realm::Server));
    }

    #[test]
    fn a_dependency_value_splits_into_scope_name_and_requirement() {
        let dep = parse_requirement("Comm", "sleitnick/comm@^0.3").unwrap();
        assert_eq!(dep.alias, "Comm");
        assert_eq!(dep.scope, "sleitnick");
        assert_eq!(dep.name, "comm");
        assert!(dep
            .requirement
            .matches(&semver::Version::parse("0.3.5").unwrap()));
        assert!(!dep
            .requirement
            .matches(&semver::Version::parse("0.4.0").unwrap()));
    }

    #[test]
    fn a_malformed_dependency_value_is_dropped_not_refused() {
        assert!(parse_requirement("X", "not-a-valid-entry").is_none());
    }

    #[test]
    fn parses_the_real_wally_toml_shape_confirmed_by_hand_this_session() {
        let text = r#"
[package]
name = "sleitnick/knit"
description = "A framework"
version = "1.5.1"
license = "MIT"
registry = "https://github.com/UpliftGames/wally-index"
realm = "shared"

[dependencies]
Comm = "sleitnick/comm@^0.3"
Promise = "evaera/promise@^4"
"#;
        let manifest = parse_manifest(text).unwrap();
        assert_eq!(manifest.dependencies.len(), 2);
        assert!(manifest.dependencies.iter().any(|d| d.alias == "Comm"));
        assert!(manifest.dependencies.iter().any(|d| d.alias == "Promise"));
    }

    #[test]
    fn a_manifest_with_no_dependencies_table_parses_to_an_empty_list() {
        let text = r#"
[package]
name = "sleitnick/net"
version = "0.2.0"
realm = "shared"
"#;
        let manifest = parse_manifest(text).unwrap();
        assert!(manifest.dependencies.is_empty());
    }

    #[test]
    fn a_manifest_without_dependency_tables_parses_to_none() {
        let text = r#"
[package]
name = "x/y"
version = "1.0.0"
"#;
        let manifest = parse_manifest(text).unwrap();
        assert!(manifest.dependencies.is_empty());
    }
}
