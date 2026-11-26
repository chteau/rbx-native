//! One permission model for everything that acts in the editor on the
//! user's behalf: MCP clients (an AI assistant) and plugins, whether Roblox
//! plugins, rbx-native Luau plugins or WASM ones.
//!
//! Each of them asks for a [`Capability`]; the [`Policy`] answers with a
//! [`Decision`]. MCP clients get an access level as a starting point
//! (Studio's MCP Permissions tab: Safe, Balanced, Full access, then Custom
//! once one capability is changed) and plain allow or deny, never a prompt,
//! as in Studio. A plugin's defaults come from what it is: a Roblox plugin
//! may do what Studio lets one do, asking first for script injection and
//! for each new HTTP host; an rbx-native plugin gets what its `plugin.toml`
//! asks for and nothing else. What the user changes is stored on top.

mod capability;
mod policy;
mod store;

#[cfg(test)]
mod tests;

pub use capability::{manifest_text, parse_manifest, Capability, Request};
pub use policy::{
    Decision, Level, McpGrants, PluginGrants, PluginKind, PluginProfile, Policy, MCP_CAPABILITIES,
};
pub use store::{Shared, Store};
