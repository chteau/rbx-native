//! Which Open Cloud permissions the editor asks an API key for, and which of
//! them a given key actually holds — what the setup wizard shows as a
//! pass/fail list.
//!
//! Scope strings are the `x-roblox-scopes` names in `Roblox/creator-docs`'
//! `reference/cloud/openapi.json`, not guessed. Introspection reports them
//! split in two (`universe-places:write` comes back as name
//! `universe-places`, operation `write`), which [`check`] undoes.

use crate::introspect::KeyInfo;

/// Where a key is created: the Creator Dashboard's API Keys tab.
pub const DASHBOARD_API_KEYS_URL: &str =
    "https://create.roblox.com/dashboard/credentials?activeTab=ApiKeysTab";

/// One permission the editor can use, and what for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permission {
    /// `name:operation`, exactly as the Dashboard's operation picker and the
    /// OpenAPI spec spell it.
    pub scope: &'static str,
    /// The editor feature that stops working without it — the wizard's row
    /// label, worded as Kevin's boards (#0075) have it.
    pub feature: &'static str,
    /// Whether the editor's core loop (open a place from Roblox, save it
    /// back) needs it. Everything else only switches one feature off.
    pub required: bool,
}

const fn required(scope: &'static str, feature: &'static str) -> Permission {
    Permission {
        scope,
        feature,
        required: true,
    }
}

const fn optional(scope: &'static str, feature: &'static str) -> Permission {
    Permission {
        scope,
        feature,
        required: false,
    }
}

/// Everything the editor can use, required first.
pub const PERMISSIONS: &[Permission] = &[
    required("universe-places:write", "Save and publish places"),
    // The keyed asset-delivery endpoint `Client::download_place` falls back
    // to for a private place.
    required("legacy-asset:manage", "Open private places and assets"),
    // The Inventory API's CREATED_PLACE listing: the one way an unrestricted
    // key reaches the owner's private experiences, so without it My Games
    // silently misses them. Required, unlike the optional scopes below,
    // which only switch on features this editor doesn't have yet.
    required(
        "user.inventory-item:read",
        "List private experiences on Home",
    ),
    optional("universe.place:read", "Version history"),
    optional("universe.place:write", "Place settings and version notes"),
    optional("universe:write", "Game settings"),
    optional("universe.thumbnail:read", "Game settings thumbnails"),
    optional("legacy-group:manage", "Group experiences on Home"),
    optional("game-pass:read", "View game passes"),
    optional("game-pass:write", "Edit game passes"),
    optional("developer-product:read", "View developer products"),
    optional("developer-product:write", "Edit developer products"),
    optional("universe-datastores.control:list", "List data stores"),
    optional("universe-datastores.objects:list", "Browse data store keys"),
    optional(
        "universe-datastores.objects:read",
        "Read data store entries",
    ),
    optional(
        "universe-datastores.objects:create",
        "Create data store entries",
    ),
    optional(
        "universe-datastores.objects:update",
        "Update data store entries",
    ),
    optional(
        "universe-datastores.objects:delete",
        "Delete data store entries",
    ),
    optional(
        "universe.ordered-data-store.scope.entry:read",
        "Read ordered data stores",
    ),
    optional(
        "universe.ordered-data-store.scope.entry:write",
        "Edit ordered data stores",
    ),
    optional(
        "universe.place.luau-execution-session:read",
        "Read cloud Luau results",
    ),
    optional(
        "universe.place.luau-execution-session:write",
        "Run Luau in the cloud",
    ),
    optional("asset:read", "Read uploaded meshes and images"),
    optional("asset:write", "Upload meshes and images"),
    optional(
        "universe-messaging-service:publish",
        "Publish to MessagingService",
    ),
];

/// Whether a key holds one [`Permission`], and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grant {
    Missing,
    /// Every experience the key's owner can edit, including future ones.
    Everywhere,
    /// Restricted to these universes only.
    Universes(Vec<u64>),
}

impl Grant {
    pub fn granted(&self) -> bool {
        !matches!(self, Grant::Missing)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeCheck {
    pub permission: Permission,
    pub grant: Grant,
}

/// The whole report the wizard shows for a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyReport {
    /// `false` for a disabled or expired key: nothing below works then,
    /// whatever it says.
    pub usable: bool,
    pub checks: Vec<ScopeCheck>,
}

impl KeyReport {
    /// Whether every required permission is granted on a usable key.
    pub fn ready(&self) -> bool {
        self.usable
            && self
                .checks
                .iter()
                .all(|c| !c.permission.required || c.grant.granted())
    }
}

/// Checks `info` against every [`PERMISSIONS`] entry.
pub fn check(info: &KeyInfo) -> KeyReport {
    let checks = PERMISSIONS
        .iter()
        .map(|&permission| {
            let (name, operation) = permission
                .scope
                .rsplit_once(':')
                .expect("every PERMISSIONS scope is name:operation");
            let grant = info
                .scopes
                .iter()
                .find(|s| s.name == name && s.operations.iter().any(|o| o == operation))
                .map_or(Grant::Missing, |s| {
                    // `"*"` fails to parse and is dropped by introspect, so
                    // an unrestricted scope and a scope that cannot be
                    // restricted both arrive with no ids.
                    if s.universe_ids.is_empty() {
                        Grant::Everywhere
                    } else {
                        Grant::Universes(s.universe_ids.clone())
                    }
                });
            ScopeCheck { permission, grant }
        })
        .collect();
    KeyReport {
        usable: info.enabled && !info.expired,
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::introspect::Scope;

    fn info(scopes: Vec<Scope>) -> KeyInfo {
        KeyInfo {
            name: "k".into(),
            authorized_user_id: 1,
            scopes,
            enabled: true,
            expired: false,
            expiration_time_utc: String::new(),
        }
    }

    fn scope(name: &str, ops: &[&str], universes: &[u64]) -> Scope {
        Scope {
            name: name.into(),
            operations: ops.iter().map(|s| s.to_string()).collect(),
            universe_ids: universes.to_vec(),
        }
    }

    fn grant(report: &KeyReport, scope: &str) -> Grant {
        report
            .checks
            .iter()
            .find(|c| c.permission.scope == scope)
            .unwrap()
            .grant
            .clone()
    }

    #[test]
    fn every_scope_is_name_colon_operation_and_listed_once() {
        for (i, p) in PERMISSIONS.iter().enumerate() {
            assert!(p.scope.rsplit_once(':').is_some(), "{}", p.scope);
            assert!(
                !PERMISSIONS[..i].iter().any(|q| q.scope == p.scope),
                "{}",
                p.scope
            );
        }
    }

    #[test]
    fn the_real_test_keys_scopes_pass_and_fail_where_they_should() {
        // The live key's introspection (2026-09-25), `"*"` already dropped.
        let report = check(&info(vec![
            scope("group", &["read"], &[]),
            scope("legacy-asset", &["manage"], &[]),
            scope("universe-places", &["write"], &[]),
            scope("universe.thumbnail", &["read"], &[]),
        ]));
        // Without the Inventory scope, private games can't be listed.
        assert!(!report.ready());
        assert_eq!(grant(&report, "universe-places:write"), Grant::Everywhere);
        assert_eq!(grant(&report, "legacy-asset:manage"), Grant::Everywhere);
        assert_eq!(grant(&report, "universe.thumbnail:read"), Grant::Everywhere);
        assert_eq!(grant(&report, "game-pass:read"), Grant::Missing);
    }

    #[test]
    fn a_restricted_scope_reports_its_universes_and_operations_must_match() {
        let report = check(&info(vec![
            scope("universe-places", &["write"], &[42]),
            scope("game-pass", &["read"], &[]),
        ]));
        assert_eq!(
            grant(&report, "universe-places:write"),
            Grant::Universes(vec![42])
        );
        assert_eq!(grant(&report, "game-pass:write"), Grant::Missing);
        // legacy-asset:manage is required and absent.
        assert!(!report.ready());
    }

    #[test]
    fn an_expired_key_is_never_ready() {
        let mut key = info(vec![
            scope("universe-places", &["write"], &[]),
            scope("legacy-asset", &["manage"], &[]),
            scope("user.inventory-item", &["read"], &[]),
        ]);
        assert!(check(&key).ready());
        key.expired = true;
        assert!(!check(&key).ready());
    }
}
