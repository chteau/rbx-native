//! The resolver: a BFS over the dependency graph, matching Wally's own
//! `resolution.rs` shape — reuse an already-activated version of a
//! `scope/name` if it satisfies the new requirement, else fetch the
//! highest published version that does, and queue *its* dependencies too.
//! Every version discovery goes through [`super::search::versions_of`]
//! (an HTTP search, not a git-cloned registry index — see this crate's
//! module doc comment), and every fetch doubles as the install content
//! (`content::fetch` returns the unzipped tree alongside the manifest, so
//! nothing here is thrown away once installation begins).
//!
//! Unlike Wally's own resolver, this one doesn't insist on a single
//! version per `scope/name` across the whole graph: two requirers wanting
//! genuinely incompatible majors of the same package both get satisfied,
//! each in its own version-qualified `_Index` slot at install time (see
//! `shell::wally_sync`) — simpler than conflict resolution, and safe,
//! since the install layout already keys by exact version.

use std::collections::{HashMap, VecDeque};

use super::content::{self, Package};
use super::search;

/// One resolved package, and which exact (scope, name, version) each of
/// its own dependency aliases landed on — the sibling alias files its own
/// `_Index` entry needs at install time.
pub(crate) struct ResolvedPackage {
    pub(crate) scope: String,
    pub(crate) name: String,
    pub(crate) version: semver::Version,
    pub(crate) package: Package,
    pub(crate) dependency_edges: Vec<(String, PackageKey)>,
}

pub(crate) struct ResolvedGraph {
    /// The root pick is always `packages[0]`.
    pub(crate) packages: Vec<ResolvedPackage>,
}

pub(crate) struct ResolveError {
    pub(crate) scope: String,
    pub(crate) name: String,
    pub(crate) requirement: semver::VersionReq,
    pub(crate) latest: Option<semver::Version>,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}@{} — no published version matches",
            self.scope, self.name, self.requirement
        )?;
        match &self.latest {
            Some(latest) => write!(f, ", latest is {latest}"),
            None => write!(f, " (nothing published at all)"),
        }
    }
}

/// Resolution can't run past this many activated packages — a real
/// dependency graph never gets close; this only guards against a
/// pathological/misbehaving registry response looping the queue forever.
const MAX_PACKAGES: usize = 200;

/// One package's identity: `scope`, `name` and the exact resolved
/// `version` — the key every map in this module is keyed by, and (nested
/// one level, as the target half of a dependency edge) what
/// `ResolvedPackage::dependency_edges` points at.
type PackageKey = (String, String, semver::Version);

pub(crate) fn resolve(
    scope: &str,
    name: &str,
    root_version: semver::Version,
) -> Result<ResolvedGraph, String> {
    let mut order: Vec<PackageKey> = Vec::new();
    let mut activated: HashMap<PackageKey, Package> = HashMap::new();
    let mut queue: VecDeque<(String, String, semver::VersionReq)> = VecDeque::new();

    let root = content::fetch(scope, name, &root_version)?;
    queue.extend(
        root.manifest
            .dependencies
            .iter()
            .map(|dep| (dep.scope.clone(), dep.name.clone(), dep.requirement.clone())),
    );
    order.push((scope.to_owned(), name.to_owned(), root_version.clone()));
    activated.insert((scope.to_owned(), name.to_owned(), root_version), root);

    while let Some((req_scope, req_name, requirement)) = queue.pop_front() {
        if order.len() >= MAX_PACKAGES {
            break;
        }
        let already_satisfied = activated
            .keys()
            .any(|(s, n, v)| *s == req_scope && *n == req_name && requirement.matches(v));
        if already_satisfied {
            continue;
        }

        let versions = search::versions_of(&req_scope, &req_name)?;
        let picked = versions
            .iter()
            .rev()
            .find(|v| requirement.matches(v))
            .cloned();
        let Some(picked_version) = picked else {
            return Err(ResolveError {
                scope: req_scope,
                name: req_name,
                requirement,
                latest: versions.last().cloned(),
            }
            .to_string());
        };

        let package = content::fetch(&req_scope, &req_name, &picked_version)?;
        queue.extend(
            package
                .manifest
                .dependencies
                .iter()
                .map(|dep| (dep.scope.clone(), dep.name.clone(), dep.requirement.clone())),
        );
        order.push((req_scope.clone(), req_name.clone(), picked_version.clone()));
        activated.insert((req_scope, req_name, picked_version), package);
    }

    // Every dependency edge is resolved first, against the *whole*
    // `activated` map still intact — a diamond graph (two different
    // packages both depending on the same third one) needs every other
    // entry still present no matter which order `order` visits them in,
    // so nothing here may remove from `activated` until every edge has
    // been looked up.
    let edges: HashMap<PackageKey, Vec<(String, PackageKey)>> = order
        .iter()
        .map(|key| {
            let package = &activated[key];
            let resolved_edges = package
                .manifest
                .dependencies
                .iter()
                .filter_map(|dep| {
                    let matched = find_activated(&activated, dep)?;
                    Some((dep.alias.clone(), matched))
                })
                .collect();
            (key.clone(), resolved_edges)
        })
        .collect();

    let packages = order
        .into_iter()
        .map(|(scope, name, version)| {
            let key = (scope.clone(), name.clone(), version.clone());
            let package = activated
                .remove(&key)
                .expect("every entry in `order` was inserted into `activated` alongside it");
            let dependency_edges = edges.get(&key).cloned().unwrap_or_default();
            ResolvedPackage {
                scope,
                name,
                version,
                package,
                dependency_edges,
            }
        })
        .collect();

    Ok(ResolvedGraph { packages })
}

/// Finds which exact activated version satisfies one dependency edge.
fn find_activated(
    activated: &HashMap<PackageKey, Package>,
    dep: &content::Dependency,
) -> Option<PackageKey> {
    activated
        .keys()
        .filter(|(s, n, _)| *s == dep.scope && *n == dep.name)
        .find(|(_, _, v)| dep.requirement.matches(v))
        .map(|(s, n, v)| (s.clone(), n.clone(), v.clone()))
}

#[cfg(test)]
mod tests;
