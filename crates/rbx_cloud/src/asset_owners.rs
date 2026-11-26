//! Whose assets the editor's user may open as an `EditableMesh` or
//! `EditableImage`. Roblox lets `CreateEditableMeshAsync` and
//! `CreateEditableImageAsync` load an asset owned by the experience's
//! owner, by the signed-in Studio user, or by a group where one of them may
//! edit the group's assets (EditableMesh's "Permissions" doc).

use crate::client::Client;
use crate::error::CloudError;
use crate::universes::Owner;

/// The creators whose assets may be opened as editables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetOwners {
    pub users: Vec<u64>,
    pub groups: Vec<u64>,
}

impl AssetOwners {
    fn add(&mut self, owner: Owner) {
        let (list, id) = match owner {
            Owner::User(id) => (&mut self.users, id),
            Owner::Group(id) => (&mut self.groups, id),
        };
        if !list.contains(&id) {
            list.push(id);
        }
    }
}

impl Client {
    /// The key's user and the groups it may manage, plus the owner of
    /// `universe` (0 for a place that isn't published). The groups need
    /// `legacy-group:manage`, the one listing Open Cloud has of them: a key
    /// without it reaches no group's assets. No key: only the experience's
    /// owner.
    pub fn asset_owners(&self, universe: u64) -> Result<AssetOwners, CloudError> {
        let mut owners = AssetOwners::default();
        if self.require_api_key().is_ok() {
            let info = self.introspect()?;
            owners.add(Owner::User(info.authorized_user_id));
            let manages_groups = info
                .scopes
                .iter()
                .any(|s| s.name == "legacy-group" && s.operations.iter().any(|o| o == "manage"));
            if manages_groups {
                for group in self.manageable_groups()? {
                    owners.add(Owner::Group(group.id));
                }
            }
        }
        if universe != 0 {
            // A key reaches only its own experiences; anyone else's owner
            // comes from the public games listing.
            let owner = match self.universe(universe) {
                Ok(found) => found.owner,
                Err(_) => self.experience_creator(universe)?,
            };
            owners.add(owner);
        }
        Ok(owners)
    }
}

#[cfg(test)]
mod tests {
    use crate::datastores::tests::{client, serve};

    const INTROSPECT: &str = r#"{"name":"k","authorizedUserId":925308243,"scopes":[{"name":"legacy-group","operations":["manage"]}],"enabled":true,"expirationTimeUtc":"","expired":false}"#;

    #[test]
    fn the_user_their_groups_and_the_experience_owner() {
        let (root, log) = serve(vec![
            (200, INTROSPECT),
            (200, r#"{"data":[{"id":7,"name":"Studio Team"}]}"#),
            (
                200,
                r#"{"path":"universes/42","displayName":"G","description":"","group":"groups/9","visibility":"PUBLIC","rootPlace":"universes/42/places/5"}"#,
            ),
        ]);
        let owners = client(&root).asset_owners(42).unwrap();
        assert_eq!(owners.users, [925308243]);
        assert_eq!(owners.groups, [7, 9]);
        let paths: Vec<String> = log
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.path().to_owned())
            .collect();
        assert_eq!(
            paths,
            [
                "/api-keys/v1/introspect",
                "/legacy-develop/v1/user/groups/canmanage",
                "/cloud/v2/universes/42"
            ]
        );
    }

    #[test]
    fn without_the_group_scope_only_the_user_and_no_universe_call_unpublished() {
        let (root, log) = serve(vec![(
            200,
            r#"{"name":"k","authorizedUserId":5,"scopes":[],"enabled":true,"expirationTimeUtc":"","expired":false}"#,
        )]);
        let owners = client(&root).asset_owners(0).unwrap();
        assert_eq!(owners.users, [5]);
        assert!(owners.groups.is_empty());
        assert_eq!(log.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_refused_key_is_an_error_not_an_empty_list() {
        let (root, _log) = serve(vec![(401, r#"{"message":"Invalid API Key"}"#)]);
        assert!(client(&root).asset_owners(0).is_err());
    }
}
