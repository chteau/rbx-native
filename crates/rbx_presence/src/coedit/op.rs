//! The operation model, shared byte-for-byte with the Studio plugin
//! (`plugins/team-presence/src/Coedit*.luau`) and pinned by
//! `plugins/team-presence/fixtures/coedit_ops.json`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A Lamport clock: `(counter, actor)`, compared counter first, then actor
/// (plain string order). Serialized as the JSON array `[counter, "actor"]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Clock(pub u64, pub String);

/// An instance as an op names it: its id (empty when the sender does not
/// know one, as the Studio plugin for an instance it did not create) and its
/// path ([`crate::paths`]) on the sender's side when the op was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub p: String,
}

/// One edit. JSON: an object with `"k"` = `set` | `add` | `del` | `src`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum Op {
    /// A property write. `n` is the property name; `Name` is the instance
    /// name and `Parent` its parent (value a `Ref`). `v` is a
    /// [`crate::coedit::value`] JSON value, `null` to remove the property.
    Set {
        #[serde(flatten)]
        at: Target,
        n: String,
        v: Value,
        c: Clock,
    },
    /// A new instance with every property it has (`Name` included; a script's
    /// `Source` included), under `parent` (`None` = root).
    Add {
        #[serde(flatten)]
        at: Target,
        cls: String,
        parent: Option<Target>,
        props: serde_json::Map<String, Value>,
        c: Clock,
    },
    /// The instance and its subtree are deleted.
    Del {
        #[serde(flatten)]
        at: Target,
        c: Clock,
    },
    /// A script's whole `Source`, written against the log text whose
    /// [`crate::coedit::merge::text_hash`] is `base`.
    Src {
        #[serde(flatten)]
        at: Target,
        base: String,
        text: String,
        c: Clock,
    },
}

/// What one editor sends at once: `a` is the sender's actor
/// (`"<userId>:<client word>:<session>"`, the session a random word so one
/// user in two editors is two actors), the same string its clocks carry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub a: String,
    pub ops: Vec<Op>,
}

impl Op {
    pub fn target(&self) -> &Target {
        match self {
            Op::Set { at, .. } | Op::Add { at, .. } | Op::Del { at, .. } | Op::Src { at, .. } => at,
        }
    }
    pub fn clock(&self) -> &Clock {
        match self {
            Op::Set { c, .. } | Op::Add { c, .. } | Op::Del { c, .. } | Op::Src { c, .. } => c,
        }
    }
}
