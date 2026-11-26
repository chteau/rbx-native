//! The policy and the file it lives in, shared by the editor's settings and
//! the plugin host, both on the UI thread.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::Policy;

/// The one store every check reads, behind `Rc<RefCell<..>>`.
pub type Shared = Rc<RefCell<Store>>;

#[derive(Debug, Default)]
pub struct Store {
    policy: Policy,
    path: Option<PathBuf>,
}

impl Store {
    /// Reads `path`; a missing or unreadable file is the defaults (and is
    /// replaced on the first change).
    pub fn open(path: PathBuf) -> Self {
        let policy = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Store {
            policy,
            path: Some(path),
        }
    }

    /// Kept in memory only (tests, a headless run).
    pub fn in_memory(policy: Policy) -> Self {
        Store { policy, path: None }
    }

    pub fn shared(self) -> Shared {
        Rc::new(RefCell::new(self))
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Applies `change` and writes the file when it changed anything.
    pub fn change(&mut self, change: impl FnOnce(&mut Policy)) -> Result<(), String> {
        let before = self.policy.clone();
        change(&mut self.policy);
        if self.policy == before {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = serde_json::to_string_pretty(&self.policy).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}
