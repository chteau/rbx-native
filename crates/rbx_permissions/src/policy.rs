//! What each agent may do: defaults by who it is, the user's changes on
//! top.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Capability;

/// The answer to "may it?". `Ask` prompts the user the first time, where
/// the editor prompts (a Roblox plugin's script injection and HTTP hosts);
/// on `Network` it means "decided host by host".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Ask,
    Deny,
}

/// An MCP client's starting point, Studio's access levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Safe,
    #[default]
    Balanced,
    Full,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Safe, Level::Balanced, Level::Full];

    pub fn label(self) -> &'static str {
        match self {
            Level::Safe => "Safe",
            Level::Balanced => "Balanced",
            Level::Full => "Full access",
        }
    }

    /// Safe only looks; Balanced also edits and runs code in the editor;
    /// Full access adds what leaves the editor for good (uploads, files).
    pub fn grants(self, capability: Capability) -> bool {
        use Capability::*;
        match self {
            Level::Safe => capability == ReadPlace,
            Level::Balanced => matches!(capability, ReadPlace | EditPlace | EditScripts | RunCode),
            Level::Full => true,
        }
    }
}

/// The capabilities an MCP client's tools can need.
pub const MCP_CAPABILITIES: [Capability; 6] = [
    Capability::ReadPlace,
    Capability::EditPlace,
    Capability::EditScripts,
    Capability::RunCode,
    Capability::CloudUpload,
    Capability::Files,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PluginKind {
    #[default]
    Roblox,
    Luau,
    Wasm,
}

impl PluginKind {
    pub fn label(self) -> &'static str {
        match self {
            PluginKind::Roblox => "Roblox plugin",
            PluginKind::Luau => "rbx-native plugin",
            PluginKind::Wasm => "WASM plugin",
        }
    }
}

/// A plugin as the policy needs to know it: what it is and what its
/// `plugin.toml` asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginProfile {
    pub name: String,
    pub kind: PluginKind,
    pub requested: Vec<Capability>,
    pub hosts: Vec<String>,
}

impl PluginProfile {
    pub fn roblox(name: &str) -> Self {
        PluginProfile {
            name: name.to_owned(),
            ..Default::default()
        }
    }

    /// What the editor can actually hold this kind of plugin to. A Luau
    /// plugin always reads the place (in Studio too: `game` is the way to
    /// everything else); WASM has no network, debugger or HTTP calls.
    pub fn capabilities(&self) -> &'static [Capability] {
        use Capability::*;
        match self.kind {
            PluginKind::Roblox | PluginKind::Luau => {
                &[EditPlace, EditScripts, Selection, RunCode, Network]
            }
            PluginKind::Wasm => &[ReadPlace, EditPlace, EditScripts, Selection],
        }
    }

    pub fn default_decision(&self, capability: Capability) -> Decision {
        match self.kind {
            PluginKind::Roblox => match capability {
                Capability::EditScripts | Capability::Network => Decision::Ask,
                _ => Decision::Allow,
            },
            PluginKind::Luau | PluginKind::Wasm => {
                if self.requested.contains(&capability) {
                    Decision::Allow
                } else if capability == Capability::Network {
                    Decision::Ask
                } else {
                    Decision::Deny
                }
            }
        }
    }

    /// A host nobody answered for yet: allowed when the manifest lists it;
    /// asked for a Roblox plugin; refused for an rbx-native one, whose
    /// manifest is the whole list.
    pub fn default_host(&self, host: &str) -> Decision {
        if self.hosts.iter().any(|h| h == host) {
            Decision::Allow
        } else if self.kind == PluginKind::Roblox {
            Decision::Ask
        } else {
            Decision::Deny
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpGrants {
    #[serde(default)]
    pub level: Level,
    /// Only what differs from `level`.
    #[serde(default)]
    pub overrides: BTreeMap<Capability, bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginGrants {
    #[serde(default)]
    pub decisions: BTreeMap<Capability, Decision>,
    /// Answers for single HTTP hosts.
    #[serde(default)]
    pub hosts: BTreeMap<String, bool>,
}

/// Everything the user decided, by agent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub mcp: McpGrants,
    /// By plugin name.
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginGrants>,
}

impl Policy {
    pub fn mcp_allows(&self, capability: Capability) -> bool {
        self.mcp
            .overrides
            .get(&capability)
            .copied()
            .unwrap_or_else(|| self.mcp.level.grants(capability))
    }

    /// The level the MCP grants match exactly, `None` for Custom.
    pub fn mcp_level(&self) -> Option<Level> {
        Level::ALL.into_iter().find(|level| {
            MCP_CAPABILITIES
                .iter()
                .all(|c| level.grants(*c) == self.mcp_allows(*c))
        })
    }

    pub fn set_mcp_level(&mut self, level: Level) {
        self.mcp = McpGrants {
            level,
            overrides: BTreeMap::new(),
        };
    }

    pub fn set_mcp(&mut self, capability: Capability, allow: bool) {
        if self.mcp.level.grants(capability) == allow {
            self.mcp.overrides.remove(&capability);
        } else {
            self.mcp.overrides.insert(capability, allow);
        }
    }

    /// The decision the user stored, if any.
    pub fn stored(&self, plugin: &str, capability: Capability) -> Option<Decision> {
        self.plugins
            .get(plugin)?
            .decisions
            .get(&capability)
            .copied()
    }

    pub fn decide(&self, profile: &PluginProfile, capability: Capability) -> Decision {
        self.stored(&profile.name, capability)
            .unwrap_or_else(|| profile.default_decision(capability))
    }

    pub fn allows(&self, profile: &PluginProfile, capability: Capability) -> bool {
        self.decide(profile, capability) == Decision::Allow
    }

    /// `Network` decides first: Allow or Deny covers every host, Ask leaves
    /// it to the host's own answer.
    pub fn host(&self, profile: &PluginProfile, host: &str) -> Decision {
        match self.decide(profile, Capability::Network) {
            Decision::Ask => match self.stored_host(&profile.name, host) {
                Some(true) => Decision::Allow,
                Some(false) => Decision::Deny,
                None => profile.default_host(host),
            },
            whole => whole,
        }
    }

    pub fn stored_host(&self, plugin: &str, host: &str) -> Option<bool> {
        self.plugins.get(plugin)?.hosts.get(host).copied()
    }

    /// `None` puts the capability back to the plugin's default.
    pub fn set(&mut self, plugin: &str, capability: Capability, decision: Option<Decision>) {
        let entry = self.plugins.entry(plugin.to_owned()).or_default();
        match decision {
            Some(decision) => entry.decisions.insert(capability, decision),
            None => entry.decisions.remove(&capability),
        };
        self.prune(plugin);
    }

    /// `None` forgets the answer for `host`.
    pub fn set_host(&mut self, plugin: &str, host: &str, allow: Option<bool>) {
        let entry = self.plugins.entry(plugin.to_owned()).or_default();
        match allow {
            Some(allow) => entry.hosts.insert(host.to_owned(), allow),
            None => entry.hosts.remove(host),
        };
        self.prune(plugin);
    }

    /// The hosts to list for a plugin: its manifest's, then every one it
    /// was answered for.
    pub fn hosts(&self, profile: &PluginProfile) -> Vec<String> {
        let mut hosts = profile.hosts.clone();
        if let Some(grants) = self.plugins.get(&profile.name) {
            for host in grants.hosts.keys() {
                if !hosts.contains(host) {
                    hosts.push(host.clone());
                }
            }
        }
        hosts
    }

    fn prune(&mut self, plugin: &str) {
        if self
            .plugins
            .get(plugin)
            .is_some_and(|g| g.decisions.is_empty() && g.hosts.is_empty())
        {
            self.plugins.remove(plugin);
        }
    }
}
