//! `POST /api-keys/v1/introspect`: who does this API key belong to, and
//! which restricted universes (if any) can it see.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{self, CloudError};

const INTROSPECT_URL: &str = "https://apis.roblox.com/api-keys/v1/introspect";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    pub name: String,
    pub authorized_user_id: u64,
    pub scopes: Vec<Scope>,
    pub enabled: bool,
    pub expired: bool,
    pub expiration_time_utc: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub name: String,
    pub operations: Vec<String>,
    /// Present only on scopes restricted to specific universes; these are
    /// the only way to discover *private* experiences an API key can reach.
    pub universe_ids: Vec<u64>,
}

#[derive(Deserialize)]
struct KeyInfoRaw {
    name: String,
    #[serde(rename = "authorizedUserId")]
    authorized_user_id: u64,
    scopes: Vec<ScopeRaw>,
    enabled: bool,
    expired: bool,
    /// Absent or `null` on a key with no expiration.
    #[serde(rename = "expirationTimeUtc", default)]
    expiration_time_utc: Option<String>,
}

#[derive(Deserialize)]
struct ScopeRaw {
    name: String,
    operations: Vec<String>,
    #[serde(rename = "universeIds", default)]
    universe_ids: Vec<String>,
}

#[derive(Serialize)]
struct IntrospectRequest<'a> {
    #[serde(rename = "apiKey")]
    api_key: &'a str,
}

impl From<KeyInfoRaw> for KeyInfo {
    fn from(raw: KeyInfoRaw) -> Self {
        KeyInfo {
            name: raw.name,
            authorized_user_id: raw.authorized_user_id,
            scopes: raw.scopes.into_iter().map(Scope::from).collect(),
            enabled: raw.enabled,
            expired: raw.expired,
            expiration_time_utc: raw.expiration_time_utc.unwrap_or_default(),
        }
    }
}

impl From<ScopeRaw> for Scope {
    fn from(raw: ScopeRaw) -> Self {
        Scope {
            name: raw.name,
            operations: raw.operations,
            // Roblox encodes these as strings even though they're numeric ids;
            // a value that fails to parse is dropped rather than failing the
            // whole scope over one malformed id.
            universe_ids: raw
                .universe_ids
                .iter()
                .filter_map(|s| s.parse().ok())
                .collect(),
        }
    }
}

impl Client {
    pub fn introspect(&self) -> Result<KeyInfo, CloudError> {
        let key = self.require_api_key()?;
        let body = IntrospectRequest {
            api_key: key.as_str(),
        };
        let response = self.post_json_raw(INTROSPECT_URL, true, &body)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                INTROSPECT_URL,
                response.status,
                &response.headers,
            ));
        }
        let raw: KeyInfoRaw = serde_json::from_slice(&response.body)?;
        Ok(raw.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"name":"TestRust","authorizedUserId":925308243,"scopes":[{"name":"group","operations":["read"]},{"name":"legacy-asset","operations":["manage"]},{"name":"universe-places","operations":["write"],"universeIds":["6053515322"]}],"enabled":true,"expirationTimeUtc":"2026-10-12T21:50:06.9160000Z","expired":false}"#;

    #[test]
    fn parses_the_real_introspect_payload() {
        let raw: KeyInfoRaw = serde_json::from_str(SAMPLE).unwrap();
        let info: KeyInfo = raw.into();

        assert_eq!(info.name, "TestRust");
        assert_eq!(info.authorized_user_id, 925308243);
        assert!(info.enabled);
        assert!(!info.expired);
        assert_eq!(info.expiration_time_utc, "2026-10-12T21:50:06.9160000Z");
        assert_eq!(info.scopes.len(), 3);
    }

    #[test]
    fn a_key_without_an_expiration_parses() {
        let json = SAMPLE.replace(
            r#""expirationTimeUtc":"2026-10-12T21:50:06.9160000Z","#,
            r#""expirationTimeUtc":null,"#,
        );
        let info: KeyInfo = serde_json::from_str::<KeyInfoRaw>(&json).unwrap().into();
        assert_eq!(info.expiration_time_utc, "");
    }

    #[test]
    fn only_restricted_scopes_carry_universe_ids() {
        let raw: KeyInfoRaw = serde_json::from_str(SAMPLE).unwrap();
        let info: KeyInfo = raw.into();

        let group_scope = info.scopes.iter().find(|s| s.name == "group").unwrap();
        assert!(group_scope.universe_ids.is_empty());

        let places_scope = info
            .scopes
            .iter()
            .find(|s| s.name == "universe-places")
            .unwrap();
        assert_eq!(places_scope.universe_ids, vec![6053515322]);
        assert_eq!(places_scope.operations, vec!["write".to_string()]);
    }

    #[test]
    fn unparseable_universe_ids_are_dropped_not_fatal() {
        let raw = ScopeRaw {
            name: "universe-places".to_string(),
            operations: vec!["write".to_string()],
            universe_ids: vec!["123".to_string(), "not-a-number".to_string()],
        };
        let scope: Scope = raw.into();
        assert_eq!(scope.universe_ids, vec![123]);
    }
}
