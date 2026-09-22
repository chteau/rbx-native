use std::collections::HashMap;

use crate::wally_client::content::{Dependency, Manifest, Package, Realm};
use crate::wally_client::tree::PackageNode;

use super::*;

fn empty_package(name: &str) -> Package {
    Package {
        manifest: Manifest {
            realm: Realm::Shared,
            dependencies: Vec::new(),
        },
        tree: PackageNode {
            name: name.to_owned(),
            source: Some(String::new()),
            children: Vec::new(),
        },
    }
}

fn dependency(alias: &str, scope: &str, name: &str, req: &str) -> Dependency {
    Dependency {
        alias: alias.to_owned(),
        scope: scope.to_owned(),
        name: name.to_owned(),
        requirement: semver::VersionReq::parse(req).unwrap(),
    }
}

fn version(text: &str) -> semver::Version {
    semver::Version::parse(text).unwrap()
}

#[test]
fn finds_the_activated_version_satisfying_the_requirement() {
    let mut activated = HashMap::new();
    activated.insert(
        ("sleitnick".to_owned(), "comm".to_owned(), version("1.0.1")),
        empty_package("comm"),
    );
    let dep = dependency("Comm", "sleitnick", "comm", "^1.0");
    let found = find_activated(&activated, &dep).expect("1.0.1 satisfies ^1.0");
    assert_eq!(found.2, version("1.0.1"));
}

#[test]
fn does_not_match_an_activated_version_of_a_different_name() {
    let mut activated = HashMap::new();
    activated.insert(
        ("sleitnick".to_owned(), "net".to_owned(), version("0.2.0")),
        empty_package("net"),
    );
    let dep = dependency("Comm", "sleitnick", "comm", "^1.0");
    assert!(find_activated(&activated, &dep).is_none());
}

#[test]
fn does_not_match_an_activated_version_that_fails_the_requirement() {
    // The real sleitnick/comm@^0.3 case found by hand this session: only
    // 1.0.1 is published, which does not satisfy a ^0.3 requirement.
    let mut activated = HashMap::new();
    activated.insert(
        ("sleitnick".to_owned(), "comm".to_owned(), version("1.0.1")),
        empty_package("comm"),
    );
    let dep = dependency("Comm", "sleitnick", "comm", "^0.3");
    assert!(find_activated(&activated, &dep).is_none());
}

#[test]
fn a_diamond_dependency_resolves_to_the_one_shared_activation() {
    // Two different packages (A and B) both depend on the same `shared`
    // package — this is exactly the case the removed-too-early bug this
    // session caught would have silently dropped for whichever of A/B
    // came later in resolution order.
    let mut activated = HashMap::new();
    activated.insert(
        ("scope".to_owned(), "shared".to_owned(), version("2.0.0")),
        empty_package("shared"),
    );
    let dep_from_a = dependency("Shared", "scope", "shared", "^2.0");
    let dep_from_b = dependency("Shared", "scope", "shared", ">=1.5");
    let found_a = find_activated(&activated, &dep_from_a).unwrap();
    let found_b = find_activated(&activated, &dep_from_b).unwrap();
    assert_eq!(
        found_a, found_b,
        "both requirers land on the same activation"
    );
}

#[test]
fn resolve_error_names_the_exact_unsatisfiable_requirement() {
    let error = ResolveError {
        scope: "sleitnick".to_owned(),
        name: "comm".to_owned(),
        requirement: semver::VersionReq::parse("^0.3").unwrap(),
        latest: Some(version("1.0.1")),
    };
    let message = error.to_string();
    assert!(message.contains("sleitnick/comm"));
    assert!(message.contains("^0.3"));
    assert!(message.contains("1.0.1"));
}
