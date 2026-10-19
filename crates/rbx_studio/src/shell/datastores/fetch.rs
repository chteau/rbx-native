//! The window's network glue, blocking and run off the UI thread.
//!
//! `RBX_STUDIO_DATASTORE_MOCK=fixture[,live][,deny=<scope>]*[,throttle=<n>][,name=<text>]`
//! serves a fake universe from a local mock Open Cloud
//! (`rbx_cloud::datastore_mock`), started once per process; nothing
//! touches the network. `live` badges the universe as live, `deny=` makes
//! the mock refuse a scope, `throttle=` answers every n-th request with a
//! 429, and `name=` sets the experience name.

use std::sync::OnceLock;

use rbx_cloud::datastore_mock::{self, MockOptions, MOCK_UNIVERSE_ID};
use rbx_cloud::{ApiKey, Client, CloudError};

use super::state::{Problem, Universe, UniverseSource};

const MOCK_VARIABLE: &str = "RBX_STUDIO_DATASTORE_MOCK";

struct MockSpec {
    options: MockOptions,
    live: bool,
    name: Option<String>,
}

/// `None` unless the first item is `fixture`; unknown items are ignored.
fn parse_mock(spec: &str) -> Option<MockSpec> {
    let mut items = spec.split(',').map(str::trim);
    if items.next() != Some("fixture") {
        return None;
    }
    let mut parsed = MockSpec {
        options: MockOptions::default(),
        live: false,
        name: None,
    };
    for item in items {
        if item == "live" {
            parsed.live = true;
        } else if let Some(scope) = item.strip_prefix("deny=") {
            parsed.options.denied_scopes.push(scope.to_string());
        } else if let Some(n) = item.strip_prefix("throttle=") {
            parsed.options.throttle_every = n.parse().ok();
        } else if let Some(name) = item.strip_prefix("name=") {
            parsed.name = Some(name.to_string());
        }
    }
    Some(parsed)
}

fn mock_spec() -> Option<MockSpec> {
    parse_mock(&std::env::var(MOCK_VARIABLE).ok()?)
}

/// A client for the mock or for the stored API key.
pub(super) fn client() -> Result<Client, Problem> {
    static ROOT: OnceLock<String> = OnceLock::new();
    if let Some(spec) = mock_spec() {
        let root = ROOT.get_or_init(|| datastore_mock::start(spec.options));
        return Ok(Client::new(Some(ApiKey::new("mock"))).with_apis_root(root.clone()));
    }
    ApiKey::from_env_or_config()
        .map(|key| Client::new(Some(key)))
        .ok_or(Problem::NoKey)
}

pub(super) fn mock_universe() -> Option<Universe> {
    let spec = mock_spec()?;
    Some(Universe {
        id: MOCK_UNIVERSE_ID,
        name: Some(
            spec.name
                .unwrap_or_else(|| "Pebble Kart (mock)".to_string()),
        ),
        source: UniverseSource::Mock,
        live: spec.live,
    })
}

pub(super) fn problem(err: &CloudError) -> Problem {
    match err {
        CloudError::MissingScope { scope, .. } => Problem::MissingScope {
            scope: scope.clone(),
        },
        CloudError::Conflict { message } => Problem::Conflict(message.clone()),
        CloudError::RateLimited { .. } => Problem::RateLimited,
        CloudError::Refused {
            status: 404,
            message,
        } => Problem::NotFound(message.clone()),
        CloudError::Http { status: 404, .. } => Problem::NotFound("Not found.".to_string()),
        CloudError::NoApiKey => Problem::NoKey,
        CloudError::Refused { status: 401, .. } | CloudError::Http { status: 401, .. } => {
            Problem::Other(
                "Roblox refused the API key. Check it in Home \u{203a} Manage key.".to_string(),
            )
        }
        _ => Problem::Other(err.to_string()),
    }
}

pub(super) fn describe(problem: &Problem) -> String {
    match problem {
        Problem::MissingScope { scope } => format!(
            "The API key lacks the {scope} permission, or access to this experience. Add it in Home \u{203a} Manage key."
        ),
        Problem::NoKey => {
            "No Open Cloud API key is set up. Add one from Home \u{203a} Manage key.".to_string()
        }
        Problem::RateLimited => {
            "Roblox is rate limiting this experience; wait a moment and try again.".to_string()
        }
        Problem::Conflict(message) => format!("It changed since it was read. {message}"),
        Problem::NotFound(message) | Problem::Other(message) => message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mock_needs_fixture_first() {
        assert!(parse_mock("live").is_none());
        assert!(parse_mock("").is_none());
        let plain = parse_mock("fixture").unwrap();
        assert!(!plain.live && plain.name.is_none());
        assert!(plain.options.denied_scopes.is_empty());
    }

    #[test]
    fn parse_mock_reads_every_option() {
        let spec = parse_mock("fixture,live,deny=a:b,deny=c:d,throttle=3,name=My Game").unwrap();
        assert!(spec.live);
        assert_eq!(spec.options.denied_scopes, vec!["a:b", "c:d"]);
        assert_eq!(spec.options.throttle_every, Some(3));
        assert_eq!(spec.name.as_deref(), Some("My Game"));
    }

    #[test]
    fn problem_sorts_errors() {
        let scope = CloudError::MissingScope {
            scope: "s:r".into(),
            message: "no".into(),
        };
        assert_eq!(
            problem(&scope),
            Problem::MissingScope {
                scope: "s:r".into()
            }
        );
        let conflict = CloudError::Conflict {
            message: "m".into(),
        };
        assert_eq!(problem(&conflict), Problem::Conflict("m".into()));
        let missing = CloudError::Refused {
            status: 404,
            message: "gone".into(),
        };
        assert_eq!(problem(&missing), Problem::NotFound("gone".into()));
    }

    #[test]
    fn describe_is_one_sentence_per_problem() {
        assert!(describe(&Problem::MissingScope {
            scope: "x:y".into()
        })
        .contains("x:y"));
        assert!(describe(&Problem::NoKey).starts_with("No Open Cloud API key"));
        assert!(describe(&Problem::RateLimited).contains("rate limiting"));
        assert_eq!(
            describe(&Problem::Conflict("Retry.".into())),
            "It changed since it was read. Retry."
        );
        assert_eq!(describe(&Problem::Other("boom".into())), "boom");
    }
}
