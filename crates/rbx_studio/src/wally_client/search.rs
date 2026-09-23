//! `GET /v1/package-search` — plain JSON, no auth. The one HTTP call this
//! module makes twice for two different jobs: a free-text search for the
//! dock's dropdown, and (filtered client-side to an exact `scope`/`name`)
//! the resolver's only way to enumerate one package's published versions —
//! see this crate's module doc comment for why that's enough to skip a
//! git-cloned registry index.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SearchResult {
    pub(crate) scope: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) versions: Vec<String>,
}

pub(crate) fn search(query: &str) -> Result<Vec<SearchResult>, String> {
    let mut response = super::agent()
        .get(format!("{}/package-search", super::base()))
        .query("query", query)
        .call()
        .map_err(|err| err.to_string())?;
    response
        .body_mut()
        .read_json::<Vec<SearchResult>>()
        .map_err(|err| err.to_string())
}

/// Every published version of one exact `scope/name`, sorted ascending —
/// the resolver's own version-discovery step, standing in for a real
/// registry index clone (see the module-level doc comment on why this is
/// close enough for this client, not full parity).
pub(crate) fn versions_of(scope: &str, name: &str) -> Result<Vec<semver::Version>, String> {
    Ok(versions_from_results(search(name)?, scope, name))
}

/// The pure half of [`versions_of`], split out so the filter/sort logic is
/// testable without a live server: `search`'s own results aren't scoped to
/// one exact package (the query is a free-text match on name), so this is
/// where that narrowing actually happens.
fn versions_from_results(
    results: Vec<SearchResult>,
    scope: &str,
    name: &str,
) -> Vec<semver::Version> {
    let mut versions: Vec<semver::Version> = results
        .into_iter()
        .filter(|result| result.scope == scope && result.name == name)
        .flat_map(|result| result.versions)
        .filter_map(|version| semver::Version::parse(&version).ok())
        .collect();
    versions.sort();
    versions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(scope: &str, name: &str, versions: &[&str]) -> SearchResult {
        SearchResult {
            scope: scope.to_owned(),
            name: name.to_owned(),
            description: None,
            versions: versions.iter().map(|v| v.to_owned().to_owned()).collect(),
        }
    }

    #[test]
    fn search_results_decode_from_the_real_response_shape() {
        let json =
            r#"[{"description":"desc","name":"net","scope":"sleitnick","versions":["0.2.0"]}]"#;
        let results: Vec<SearchResult> = serde_json::from_str(json).unwrap();
        assert_eq!(results[0].scope, "sleitnick");
        assert_eq!(results[0].name, "net");
        assert_eq!(results[0].description.as_deref(), Some("desc"));
    }

    #[test]
    fn a_null_description_decodes_to_none() {
        let json = r#"[{"description":null,"name":"net","scope":"x","versions":[]}]"#;
        let results: Vec<SearchResult> = serde_json::from_str(json).unwrap();
        assert!(results[0].description.is_none());
    }

    #[test]
    fn versions_from_results_narrows_to_the_exact_scope_and_name() {
        let results = vec![
            result("sleitnick", "net", &["0.2.0"]),
            result("someone-else", "net", &["9.9.9"]),
            result("sleitnick", "comm", &["1.0.1"]),
        ];
        let versions = versions_from_results(results, "sleitnick", "net");
        assert_eq!(versions, vec![semver::Version::parse("0.2.0").unwrap()]);
    }

    #[test]
    fn versions_from_results_sorts_by_real_semver_not_string_order() {
        // String order would put "0.10.0" before "0.9.0" — real version
        // order must not.
        let results = vec![result("scope", "pkg", &["0.10.0", "0.9.0", "0.2.0"])];
        let versions = versions_from_results(results, "scope", "pkg");
        assert_eq!(
            versions,
            vec![
                semver::Version::parse("0.2.0").unwrap(),
                semver::Version::parse("0.9.0").unwrap(),
                semver::Version::parse("0.10.0").unwrap(),
            ]
        );
    }
}
