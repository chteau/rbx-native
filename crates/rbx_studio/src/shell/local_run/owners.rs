//! Whose assets `CreateEditableMeshAsync` and `CreateEditableImageAsync`
//! may open, in Edit and in Play (local): the editor's signed-in user, the
//! groups their key may manage and the experience's owner, as Studio checks
//! it. Each answer is kept on disk, so an offline run still knows its
//! user; offline with nothing kept, the check can't pass, as Studio with
//! no network can't load the asset at all.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rbx_lua::server::{AssetOwners, Creators};

/// How long an answer is trusted before asking again: a sign-in or a
/// group change shows up within this.
const FRESH: Duration = Duration::from_secs(300);

#[derive(Debug, Default)]
pub(in crate::shell) struct StudioOwners {
    known: Mutex<HashMap<u64, (Instant, Creators)>>,
    /// Where answers are kept; `None` keeps them in memory only.
    file: Option<PathBuf>,
}

/// The editor's one owner lookup, shared by Edit and every run.
pub(in crate::shell) fn shared() -> Arc<StudioOwners> {
    static SHARED: OnceLock<Arc<StudioOwners>> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            Arc::new(StudioOwners {
                known: Mutex::default(),
                file: cache_dir().map(|dir| dir.join("asset-owners.json")),
            })
        })
        .clone()
}

/// Where Edit keeps the asset details the check reads, the same place a
/// run does.
pub(in crate::shell) fn cache_dir() -> Option<PathBuf> {
    rbx_assets::cache_root().map(|root| root.join("local-run"))
}

impl StudioOwners {
    fn kept(&self) -> HashMap<String, (Vec<u64>, Vec<u64>)> {
        self.file
            .as_ref()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn keep(&self, universe: u64, creators: &Creators) {
        let Some(file) = &self.file else { return };
        let mut kept = self.kept();
        kept.insert(
            universe.to_string(),
            (creators.users.clone(), creators.groups.clone()),
        );
        if let (Some(dir), Ok(bytes)) = (file.parent(), serde_json::to_vec(&kept)) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(file, bytes);
        }
    }

    fn resolve(
        &self,
        universe: u64,
        lookup: impl FnOnce() -> Result<Creators, String>,
    ) -> Result<Creators, String> {
        let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, creators)) = known.get(&universe) {
            if at.elapsed() < FRESH {
                return Ok(creators.clone());
            }
        }
        match lookup() {
            Ok(creators) => {
                self.keep(universe, &creators);
                known.insert(universe, (Instant::now(), creators.clone()));
                Ok(creators)
            }
            Err(why) if rbx_net::is_offline_message(&why) => self
                .kept()
                .remove(&universe.to_string())
                .map(|(users, groups)| Creators { users, groups })
                .ok_or(why),
            Err(why) => Err(why),
        }
    }
}

impl AssetOwners for StudioOwners {
    fn owners(&self, universe: u64) -> Result<Creators, String> {
        self.resolve(universe, || {
            rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config())
                .asset_owners(universe)
                .map(|o| Creators {
                    users: o.users,
                    groups: o.groups,
                })
                .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rbxn_owners_{test}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("asset-owners.json")
    }

    fn owners(file: PathBuf) -> StudioOwners {
        StudioOwners {
            known: Mutex::default(),
            file: Some(file),
        }
    }

    fn creators(user: u64) -> Creators {
        Creators {
            users: vec![user],
            groups: vec![7],
        }
    }

    #[test]
    fn offline_uses_what_an_online_lookup_kept() {
        let file = scratch("offline");
        assert_eq!(
            owners(file.clone()).resolve(42, || Ok(creators(5))),
            Ok(creators(5))
        );
        let offline = || Err("offline: Offline mode is on".to_owned());
        assert_eq!(owners(file.clone()).resolve(42, offline), Ok(creators(5)));
        // Another experience was never looked up: no owners to check with.
        assert!(owners(file).resolve(43, offline).is_err());
    }

    #[test]
    fn a_refused_key_does_not_fall_back_to_what_was_kept() {
        let file = scratch("refused");
        owners(file.clone()).resolve(0, || Ok(creators(5))).unwrap();
        let refused = || Err("401 Invalid API Key".to_owned());
        assert!(owners(file).resolve(0, refused).is_err());
    }

    #[test]
    fn a_fresh_answer_is_not_asked_for_again() {
        let owners = owners(scratch("fresh"));
        owners.resolve(0, || Ok(creators(5))).unwrap();
        assert_eq!(owners.resolve(0, || panic!("asked twice")), Ok(creators(5)));
    }
}
