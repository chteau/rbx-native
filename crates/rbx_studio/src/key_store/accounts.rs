//! Several Roblox accounts' Open Cloud keys, switched without pasting a key
//! again (Settings › Account › Accounts).
//!
//! Each key lives in the OS credential store under its own entry
//! ([`slot_url`]); the entry `key_store` has always used holds a copy of
//! the active one, so everything that reads "the key" is unchanged. This
//! index is the only plaintext part (`<config>/accounts.json`): labels and
//! a fingerprint, never a secret.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// One stored key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Account {
    /// Never reused within one index.
    pub(crate) id: u32,
    /// The key owner's name, once a check has read it; empty before.
    #[serde(default)]
    pub(crate) owner: String,
    /// The key's name on Roblox, once known; empty before.
    #[serde(default)]
    pub(crate) key_name: String,
    /// [`fingerprint`] of the secret, to recognise a key saved twice.
    #[serde(default)]
    pub(crate) fingerprint: String,
}

/// The index. Pure; `key_store` does the credential-store half.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Accounts {
    #[serde(default)]
    pub(crate) accounts: Vec<Account>,
    #[serde(default)]
    pub(crate) active: Option<u32>,
    /// The next id to hand out.
    #[serde(default)]
    pub(crate) next_id: u32,
}

impl Accounts {
    /// Malformed text is an empty index; an `active` naming no account is
    /// dropped.
    pub(crate) fn from_json(text: &str) -> Accounts {
        match serde_json::from_str::<Accounts>(text) {
            Ok(mut accounts) => {
                accounts.normalize();
                accounts
            }
            Err(_) => Accounts::default(),
        }
    }

    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub(crate) fn load(path: &Path) -> Accounts {
        std::fs::read_to_string(path)
            .map(|text| Accounts::from_json(&text))
            .unwrap_or_default()
    }

    /// Temp file then rename, creating the directory.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, self.to_json())?;
        std::fs::rename(&temp, path)
    }

    pub(crate) fn get(&self, id: u32) -> Option<&Account> {
        self.accounts.iter().find(|account| account.id == id)
    }

    pub(crate) fn find(&self, fingerprint: &str) -> Option<u32> {
        self.accounts
            .iter()
            .find(|account| account.fingerprint == fingerprint)
            .map(|account| account.id)
    }

    /// The account holding `fingerprint`, added (unlabelled) if new, and
    /// made active. Returns its id and whether it was new.
    pub(crate) fn add_active(&mut self, fingerprint: &str) -> (u32, bool) {
        if let Some(id) = self.find(fingerprint) {
            self.active = Some(id);
            return (id, false);
        }
        let id = self.next_id;
        self.next_id = id.saturating_add(1);
        self.accounts.push(Account {
            id,
            owner: String::new(),
            key_name: String::new(),
            fingerprint: fingerprint.to_owned(),
        });
        self.active = Some(id);
        (id, true)
    }

    pub(crate) fn set_active(&mut self, id: u32) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        self.active = Some(id);
        true
    }

    /// Removes it; removing the active one leaves none active.
    pub(crate) fn remove(&mut self, id: u32) -> Option<Account> {
        let position = self.accounts.iter().position(|account| account.id == id)?;
        let removed = self.accounts.remove(position);
        if self.active == Some(id) {
            self.active = None;
        }
        Some(removed)
    }

    /// Names the active account; empty strings leave a field as it was.
    pub(crate) fn label_active(&mut self, owner: &str, key_name: &str) -> bool {
        let Some(id) = self.active else {
            return false;
        };
        let Some(account) = self.accounts.iter_mut().find(|account| account.id == id) else {
            return false;
        };
        if !owner.is_empty() {
            account.owner = owner.to_owned();
        }
        if !key_name.is_empty() {
            account.key_name = key_name.to_owned();
        }
        true
    }

    /// Restores the invariants a hand-edited or older file may break: ids
    /// are never handed out twice, and `active` names a stored account.
    fn normalize(&mut self) {
        let highest_next = self
            .accounts
            .iter()
            .map(|account| account.id.saturating_add(1))
            .max()
            .unwrap_or(1);
        self.next_id = self.next_id.max(highest_next);
        if let Some(id) = self.active {
            if self.get(id).is_none() {
                self.active = None;
            }
        }
    }
}

/// SHA-256 of the secret, first 16 hex digits.
pub(crate) fn fingerprint(secret: &str) -> String {
    Sha256::digest(secret.as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The credential-store entry an account's key lives under.
pub(crate) fn slot_url(id: u32) -> String {
    format!("https://apis.roblox.com/rbx-native/accounts/{id}")
}

#[cfg(test)]
mod tests;
