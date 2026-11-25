//! What an agent or a plugin may be allowed to do, and the `plugin.toml`
//! strings that ask for it.

use serde::{Deserialize, Serialize};

/// One thing an agent can do. Labels follow Studio's MCP Permissions
/// sections where one covers it, and Studio's plugin permission (Script
/// Injection) for scripts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ReadPlace,
    EditPlace,
    EditScripts,
    Selection,
    RunCode,
    Network,
    CloudUpload,
    Files,
}

impl Capability {
    pub const ALL: [Capability; 8] = [
        Capability::ReadPlace,
        Capability::EditPlace,
        Capability::EditScripts,
        Capability::Selection,
        Capability::RunCode,
        Capability::Network,
        Capability::CloudUpload,
        Capability::Files,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Capability::ReadPlace => "Look at your game",
            Capability::EditPlace => "Edit your game",
            Capability::EditScripts => "Script injection",
            Capability::Selection => "Selection",
            Capability::RunCode => "Run code & playtest",
            Capability::Network => "Connect to the internet",
            Capability::CloudUpload => "Publish & manage assets",
            Capability::Files => "Files on this computer",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Capability::ReadPlace => {
                "Inspect the place, read scripts, check the output and capture the viewport. Nothing is changed."
            }
            Capability::EditPlace => {
                "Build and change parts, UI, audio and the environment, and insert assets."
            }
            Capability::EditScripts => {
                "Insert or change scripts. Scripts can do pretty much anything once the game runs."
            }
            Capability::Selection => "Read and change what is selected.",
            Capability::RunCode => {
                "Run Luau in the editor, start and stop playtests, drive the character and use the debugger."
            }
            Capability::Network => "Send HTTP requests to the hosts you allow.",
            Capability::CloudUpload => {
                "Upload images and models to Roblox. Uploads are permanent and owned by your account."
            }
            Capability::Files => {
                "Read and write files outside the place: save a copy, import a model from disk."
            }
        }
    }

    /// The Lucide glyph the Permissions page draws beside it.
    pub fn glyph(self) -> &'static str {
        match self {
            Capability::ReadPlace => "eye",
            Capability::EditPlace => "pencil",
            Capability::EditScripts => "code",
            Capability::Selection => "mouse-pointer-2",
            Capability::RunCode => "play",
            Capability::Network => "globe",
            Capability::CloudUpload => "upload",
            Capability::Files => "folder",
        }
    }
}

/// One `plugin.toml` permission: a capability, or HTTP to one host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Capability(Capability),
    Host(String),
}

/// `dom.read`, `dom.write`, `scripts`, `selection`, `debugger`,
/// `http:<host>` (`http:*` for every host). `None` for anything else.
pub fn parse_manifest(text: &str) -> Option<Request> {
    let capability = match text {
        "dom.read" => Capability::ReadPlace,
        "dom.write" => Capability::EditPlace,
        "scripts" => Capability::EditScripts,
        "selection" => Capability::Selection,
        "debugger" => Capability::RunCode,
        "http:*" => Capability::Network,
        _ => {
            return text
                .strip_prefix("http:")
                .filter(|host| !host.is_empty())
                .map(|host| Request::Host(host.to_owned()))
        }
    };
    Some(Request::Capability(capability))
}

/// The inverse of [`parse_manifest`]; `None` for a capability no manifest
/// can ask for.
pub fn manifest_text(request: &Request) -> Option<String> {
    let text = match request {
        Request::Host(host) => return Some(format!("http:{host}")),
        Request::Capability(capability) => match capability {
            Capability::ReadPlace => "dom.read",
            Capability::EditPlace => "dom.write",
            Capability::EditScripts => "scripts",
            Capability::Selection => "selection",
            Capability::RunCode => "debugger",
            Capability::Network => "http:*",
            Capability::CloudUpload | Capability::Files => return None,
        },
    };
    Some(text.to_owned())
}
