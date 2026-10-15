//! Several Roblox accounts' Open Cloud keys, switched without pasting a key
//! again (Settings › Account › Accounts).
//!
//! Each key lives in the OS credential store under its own entry
//! ([`slot_url`]); the entry `key_store` has always used holds a copy of
//! the active one, so everything that reads "the key" is unchanged. This
//! index is the only plaintext part (`<config>/accounts.json`): labels and
//! a fingerprint, never a secret.

use std::path::Path;

/// One stored key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Account {
    /// Never reused within one index.
    pub(crate) id: u32,
    /// The key owner's name, once a check has read it; empty before.
    pub(crate) owner: String,
    /// The key's name on Roblox, once known; empty before.
    pub(crate) key_name: String,
    /// [`fingerprint`] of the secret, to recognise a key saved twice.
    pub(crate) fingerprint: String,
}

/// The index. Pure; `key_store` does the credential-store half.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Accounts {
    pub(crate) accounts: Vec<Account>,
    pub(crate) active: Option<u32>,
    /// The next id to hand out.
    pub(crate) next_id: u32,
}

impl Accounts {
    /// Malformed text is an empty index; an `active` naming no account is
    /// dropped.
    pub(crate) fn from_json(text: &str) -> Accounts {
        todo!()
    }

    pub(crate) fn to_json(&self) -> String {
        todo!()
    }

    pub(crate) fn load(path: &Path) -> Accounts {
        todo!()
    }

    /// Temp file then rename, creating the directory.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        todo!()
    }

    pub(crate) fn get(&self, id: u32) -> Option<&Account> {
        todo!()
    }

    pub(crate) fn find(&self, fingerprint: &str) -> Option<u32> {
        todo!()
    }

    /// The account holding `fingerprint`, added (unlabelled) if new, and
    /// made active. Returns its id and whether it was new.
    pub(crate) fn add_active(&mut self, fingerprint: &str) -> (u32, bool) {
        todo!()
    }

    pub(crate) fn set_active(&mut self, id: u32) -> bool {
        todo!()
    }

    /// Removes it; removing the active one leaves none active.
    pub(crate) fn remove(&mut self, id: u32) -> Option<Account> {
        todo!()
    }

    /// Names the active account; empty strings leave a field as it was.
    pub(crate) fn label_active(&mut self, owner: &str, key_name: &str) -> bool {
        todo!()
    }
}

/// SHA-256 of the secret, first 16 hex digits.
pub(crate) fn fingerprint(secret: &str) -> String {
    todo!()
}

/// The credential-store entry an account's key lives under.
pub(crate) fn slot_url(id: u32) -> String {
    todo!()
}

#[cfg(test)]
mod tests;
