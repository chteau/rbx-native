//! What the place has installed, read back from the `_Index` folders the
//! installer writes (see the parent module's doc comment): nothing about
//! installs is stored anywhere else. Every `_Index` in the place counts,
//! wherever its `Packages` folder sits, so a place a Rojo project synced
//! reads the same as one this dock filled.

use rbx_dom::{Ref, WeakDom};

use crate::wally_client::{Listing, Realm};

use super::PackageId;

/// One `<Root>/_Index/<scope>_<name>@<version>` slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct Installed {
    pub(in crate::shell) scope: String,
    pub(in crate::shell) name: String,
    pub(in crate::shell) version: semver::Version,
    /// From the folder holding `_Index`: `ServerPackages` is Server,
    /// `DevPackages` is Dev, anything else Shared.
    pub(in crate::shell) realm: Realm,
    /// The `_Index` folder itself, for an update to install beside.
    pub(in crate::shell) index: Ref,
}

impl Installed {
    pub(in crate::shell) fn id(&self) -> PackageId {
        (self.scope.clone(), self.name.clone())
    }
}

/// An installed package the registry has a newer version of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct Update {
    pub(in crate::shell) installed: Installed,
    pub(in crate::shell) latest: semver::Version,
}

/// Every installed package in `dom`: the shared ones first, then server,
/// then dev, each group by name.
pub(in crate::shell) fn installed_packages(dom: &WeakDom) -> Vec<Installed> {
    let mut found = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        if instance.name() == "_Index" {
            let realm = dom
                .parent(referent)
                .and_then(|parent| dom.get(parent))
                .map(|container| realm_of_container(container.name()))
                .unwrap_or_default();
            for &slot in instance.children() {
                let Some(slot_instance) = dom.get(slot) else {
                    continue;
                };
                if let Some((scope, name, version)) = parse_slot(slot_instance.name()) {
                    found.push(Installed {
                        scope,
                        name,
                        version,
                        realm,
                        index: referent,
                    });
                }
            }
            continue;
        }
        stack.extend(instance.children().iter().copied());
    }
    found.sort_by(|a, b| (&a.scope, &a.name, &a.version).cmp(&(&b.scope, &b.name, &b.version)));
    found
}

/// `<scope>_<name>@<version>` → its parts. A scope never holds `_`
/// (Wally's own name rule), so the first `_` is the split.
fn parse_slot(slot: &str) -> Option<(String, String, semver::Version)> {
    let (scope, rest) = slot.split_once('_')?;
    let (name, version) = rest.rsplit_once('@')?;
    if scope.is_empty() || name.is_empty() {
        return None;
    }
    let version = semver::Version::parse(version).ok()?;
    Some((scope.to_owned(), name.to_owned(), version))
}

/// The realm a `Packages`-style folder holds, by the name the installer
/// (and `wally install` itself) gives it.
fn realm_of_container(name: &str) -> Realm {
    match name {
        "ServerPackages" => Realm::Server,
        "DevPackages" => Realm::Dev,
        _ => Realm::Shared,
    }
}

/// The installed packages whose metadata names a newer version, in the
/// installed order.
pub(in crate::shell) fn updates(
    installed: &[Installed],
    metadata: impl Fn(&PackageId) -> Option<Listing>,
) -> Vec<Update> {
    installed
        .iter()
        .filter_map(|package| {
            let listing = metadata(&package.id())?;
            let latest = listing.latest().clone();
            (latest > package.version).then(|| Update {
                installed: package.clone(),
                latest,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> semver::Version {
        semver::Version::parse(text).unwrap()
    }

    fn listing(scope: &str, name: &str, versions: &[&str]) -> Listing {
        Listing {
            scope: scope.to_owned(),
            name: name.to_owned(),
            versions: versions.iter().map(|v| version(v)).collect(),
            realm: Realm::Shared,
            description: None,
        }
    }

    /// `ReplicatedStorage/Packages/_Index/...` and a root-level
    /// `ServerPackages/_Index/...`, plus a decoy `_Index` with junk in it.
    fn place() -> WeakDom {
        let mut dom = WeakDom::new();
        let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
        let packages = dom.new_instance("Folder", "Packages", Some(storage));
        let index = dom.new_instance("Folder", "_Index", Some(packages));
        dom.new_instance("Folder", "evaera_promise@4.0.0", Some(index));
        dom.new_instance("Folder", "sleitnick_signal@2.0.3", Some(index));
        dom.new_instance("Folder", "not-a-slot", Some(index));
        dom.new_instance("ModuleScript", "promise", Some(packages));
        let server = dom.new_instance("ServerPackages", "ServerPackages", None);
        let server_index = dom.new_instance("Folder", "_Index", Some(server));
        dom.new_instance("Folder", "chteau_roblox-supabase@1.2.0", Some(server_index));
        dom
    }

    #[test]
    fn reads_every_index_with_its_containers_realm() {
        let found = installed_packages(&place());
        let summary: Vec<(String, String, String, Realm)> = found
            .iter()
            .map(|p| {
                (
                    p.scope.clone(),
                    p.name.clone(),
                    p.version.to_string(),
                    p.realm,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "chteau".into(),
                    "roblox-supabase".into(),
                    "1.2.0".into(),
                    Realm::Server
                ),
                (
                    "evaera".into(),
                    "promise".into(),
                    "4.0.0".into(),
                    Realm::Shared
                ),
                (
                    "sleitnick".into(),
                    "signal".into(),
                    "2.0.3".into(),
                    Realm::Shared
                ),
            ]
        );
    }

    #[test]
    fn parse_slot_splits_on_the_first_underscore_and_the_last_at() {
        let (scope, name, version) = parse_slot("howmanysmall_janitor@1.18.3").unwrap();
        assert_eq!((scope.as_str(), name.as_str()), ("howmanysmall", "janitor"));
        assert_eq!(version, self::version("1.18.3"));
        let (_, name, _) = parse_slot("scope_two_part_name@1.0.0").unwrap();
        assert_eq!(name, "two_part_name");
        assert!(parse_slot("_name@1.0.0").is_none());
        assert!(parse_slot("scope_name").is_none());
        assert!(parse_slot("scope_name@latest").is_none());
    }

    #[test]
    fn updates_are_the_installed_packages_with_something_newer() {
        let found = installed_packages(&place());
        let known = |id: &PackageId| match (id.0.as_str(), id.1.as_str()) {
            ("evaera", "promise") => Some(listing("evaera", "promise", &["4.0.1", "4.0.0"])),
            ("sleitnick", "signal") => Some(listing("sleitnick", "signal", &["2.0.3"])),
            _ => None,
        };
        let pending = updates(&found, known);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].installed.name, "promise");
        assert_eq!(pending[0].latest, version("4.0.1"));
    }
}
