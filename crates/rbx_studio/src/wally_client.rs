//! A client for Wally (`UpliftGames/wally`, MPL-2.0)'s registry — search,
//! resolve a dependency graph, and fetch every package in it. Read-only:
//! nothing here writes anywhere, on disk or in a `WeakDom` — that's
//! `shell::wally_sync`'s job, kept separate so this module has no `Shell`/
//! `WeakDom` dependency of its own and is usable and testable in isolation
//! (the same split `argon_client` already uses).
//!
//! Confirmed by hand against the real `api.wally.run` this session:
//! `GET /v1/package-search?query=<text>` is plain JSON, no auth, and its
//! `versions` field is a package's *full* published list — enough to
//! enumerate one exact package's versions by filtering results to its
//! `scope`/`name` ([`search::versions_of`]), without cloning Wally's own
//! git-backed registry index the real CLI reads from. `GET /v1/
//! package-contents/{scope}/{name}/{version}` needs a `Wally-Version`
//! header and returns a zip of the package's *raw source tree* —
//! `wally.toml` included, and often a lot more than that (a package's
//! whole repo, tests and docs included, confirmed by hand) — Wally itself
//! never produces or touches a Roblox instance; a Rojo-style syncer does
//! that from these same files, and [`tree::PackageNode`] is this client's
//! own version of that conversion.
//!
//! [`resolve::resolve`] is the one piece with real logic: a BFS over
//! `[dependencies]`/`[server-dependencies]`, matching Wally's own
//! resolver's shape (reuse an activated version if it satisfies a new
//! requirement, else fetch the highest one that does, and recurse). See
//! that module's own doc comment for how it differs from Wally's resolver
//! and why that's safe.

mod content;
mod resolve;
mod search;
mod tree;

use std::time::Duration;

pub(crate) use content::Realm;
pub(crate) use resolve::{resolve, ResolvedGraph, ResolvedPackage};
pub(crate) use search::{search, SearchResult};
pub(crate) use tree::PackageNode;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into()
}

/// The highest published version of one exact `scope/name` — what the
/// dock resolves a clicked search result to before calling [`resolve`]
/// (this client installs latest-compatible only; see the plan's own
/// simplifications for why there's no version picker yet).
pub(crate) fn latest_version(scope: &str, name: &str) -> Result<semver::Version, String> {
    search::versions_of(scope, name)?
        .into_iter()
        .max()
        .ok_or_else(|| format!("{scope}/{name} has no published versions"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not run by default — needs the real network. Same shape as
    /// `argon_client`'s own live-server test:
    /// `cargo test -p rbx_studio --bin rbxstudio wally_client -- --ignored`.
    #[test]
    #[ignore = "needs the real api.wally.run"]
    fn searching_the_real_registry_finds_a_known_package() {
        let results = search("sleitnick/net").expect("the real registry to respond");
        assert!(results
            .iter()
            .any(|r| r.scope == "sleitnick" && r.name == "net"));
    }

    #[test]
    #[ignore = "needs the real api.wally.run"]
    fn resolving_a_dependency_free_package_installs_just_itself() {
        let version = latest_version("sleitnick", "net").expect("net has a published version");
        let graph =
            resolve("sleitnick", "net", version).expect("net has no dependencies to fail on");
        assert_eq!(graph.packages.len(), 1);
        assert_eq!(graph.packages[0].name, "net");
        assert!(graph.packages[0].dependency_edges.is_empty());
    }

    /// A real, live case found by hand this session: `sleitnick/knit
    /// @1.5.1` (an older version, still resolvable directly by exact
    /// version even though it's no longer `latest`) depends on `sleitnick/
    /// comm@^0.3`, and only `1.0.1` of comm has ever been published — an
    /// actually-broken dependency preserved in the registry's history, and
    /// a genuine exercise of the "fail the whole graph cleanly" path
    /// rather than a synthetic fixture. (The *current* `knit`, tested
    /// below, has since moved to `^1` and no longer hits this.)
    #[test]
    #[ignore = "needs the real api.wally.run, and depends on sleitnick/comm never publishing a 0.3.x"]
    fn a_real_unsatisfiable_dependency_fails_resolution_with_a_clear_message() {
        let error = match resolve(
            "sleitnick",
            "knit",
            semver::Version::parse("1.5.1").unwrap(),
        ) {
            Err(error) => error,
            Ok(_) => panic!(
                "expected comm's real ^0.3 requirement to fail against the only published 1.0.1"
            ),
        };
        assert!(error.contains("sleitnick/comm"));
        assert!(error.contains("0.3"));
    }

    /// The success path for real transitive resolution — the whole reason
    /// this pass exists over the previous "one package, no dependencies"
    /// scope. Confirmed by hand this session: `sleitnick/knit`'s current
    /// version depends on `sleitnick/comm@^1` and `evaera/promise@^4`,
    /// both satisfiable by what's actually published today.
    #[test]
    #[ignore = "needs the real api.wally.run"]
    fn resolving_a_real_package_with_dependencies_pulls_in_the_whole_graph() {
        let version = latest_version("sleitnick", "knit").expect("knit has a published version");
        let graph =
            resolve("sleitnick", "knit", version).expect("knit's current deps are satisfiable");

        assert_eq!(graph.packages[0].name, "knit", "the root pick comes first");
        let names: Vec<&str> = graph.packages.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"comm"), "expected comm among {names:?}");
        assert!(
            names.contains(&"promise"),
            "expected promise among {names:?}"
        );

        let knit = &graph.packages[0];
        assert_eq!(
            knit.dependency_edges.len(),
            2,
            "Comm and Promise both resolved"
        );
        for (alias, (dep_scope, dep_name, _)) in &knit.dependency_edges {
            match alias.as_str() {
                "Comm" => assert_eq!(
                    (dep_scope.as_str(), dep_name.as_str()),
                    ("sleitnick", "comm")
                ),
                "Promise" => assert_eq!(
                    (dep_scope.as_str(), dep_name.as_str()),
                    ("evaera", "promise")
                ),
                other => panic!("unexpected dependency alias {other}"),
            }
        }
    }
}
