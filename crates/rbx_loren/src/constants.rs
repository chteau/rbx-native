//! Every fixed name, path and pin the Loren CLI uses (`lib/constants.js`).

/// The upstream CLI version this crate is a port of; also its runtime's.
pub const CLI_VERSION: &str = "2.0.0-beta.2";
/// How to update the CLI. Upstream says `npm i -g loren-framework@next`; here
/// the CLI is part of the editor, so updating the editor is the answer.
pub const CLI_INSTALL: &str = "update RbxNative (rbxnative loren is built in)";
pub const PREFIX: &str = "(LORENঌ)";
pub const PROJECT_FILE: &str = "default.project.json";
pub const DOCS_URL: &str = "https://fabideveloper.github.io/Loren-Framework/";

pub const FOLDER_SERVICES: &str = "Services";
pub const FOLDER_CONTROLLERS: &str = "Controllers";

/// The layout's folders, keyed as upstream's `DEFAULT_PATHS` / `SCRIPT_SYNC_PATHS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Shared,
    Server,
    Client,
    Packages,
    RuntimeShared,
    RuntimeServer,
}

impl Key {
    /// DataModel path of the standard layout (`INSTANCE_PATHS`).
    pub fn instance_path(self) -> &'static [&'static str] {
        match self {
            Key::Shared => &["ReplicatedStorage", "Shared"],
            Key::Server => &["ServerScriptService", "Server"],
            Key::Client => &["StarterPlayer", "StarterPlayerScripts", "Client"],
            Key::Packages => &["ReplicatedStorage", "LorenPackages"],
            Key::RuntimeShared => &["ReplicatedStorage", "LorenRuntime"],
            Key::RuntimeServer => &["ServerScriptService", "LorenServer"],
        }
    }

    /// Where it lives on disk by default (`DEFAULT_PATHS`).
    pub fn default_path(self) -> &'static str {
        match self {
            Key::Shared => "src/shared",
            Key::Server => "src/server",
            Key::Client => "src/client",
            Key::Packages => "loren_packages",
            Key::RuntimeShared => "loren/shared",
            Key::RuntimeServer => "loren/server",
        }
    }

    /// Where it lives in a Roblox Script Sync project (`SCRIPT_SYNC_PATHS`).
    pub fn script_sync_path(self) -> &'static str {
        match self {
            Key::Shared => "ReplicatedStorage/Shared",
            Key::Server => "ServerScriptService/Server",
            Key::Client => "StarterPlayer/StarterPlayerScripts/Client",
            Key::Packages => "ReplicatedStorage/LorenPackages",
            Key::RuntimeShared => "ReplicatedStorage/Shared/LorenRuntime",
            Key::RuntimeServer => "ServerScriptService/Server/LorenServer",
        }
    }
}

pub const SCRIPT_SYNC_ID: &str = "none";
pub const SCRIPT_SYNC_LABEL: &str = "Roblox Script Sync";
pub const SCRIPT_SYNC_LAYOUT: &str = "scriptsync";
pub const LOREN_CONFIG_FILE: &str = ".loren.json";

/// The sync tools Loren runs, with their pins (`TOOLS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolPin {
    pub id: &'static str,
    pub label: &'static str,
    pub spec: &'static str,
    pub repo: &'static str,
}

pub const ROJO: ToolPin = ToolPin {
    id: "rojo",
    label: "Rojo",
    spec: "rojo-rbx/rojo@7.5.1",
    repo: "rojo-rbx/rojo",
};
pub const ARGON: ToolPin = ToolPin {
    id: "argon",
    label: "Argon",
    spec: "argon-rbx/argon@2.0.29",
    repo: "argon-rbx/argon",
};
pub const TOOLS: [ToolPin; 2] = [ROJO, ARGON];

pub fn tool_pin(id: &str) -> Option<ToolPin> {
    TOOLS.into_iter().find(|t| t.id == id)
}

/// Toolchain managers, in order of preference, with the manifest each reads.
pub const MANAGERS: [(&str, &str); 3] = [
    ("rokit", "rokit.toml"),
    ("aftman", "aftman.toml"),
    ("foreman", "foreman.toml"),
];

pub const SHIM_SOURCE: &str = "--!strict\n--!optimize 2\nlocal rt = script.Parent:FindFirstChild(\"LorenRuntime\")\n\tor game:GetService(\"ReplicatedStorage\"):WaitForChild(\"LorenRuntime\", 30)\nassert(rt, \"(LORENঌ) LorenRuntime missing (ReplicatedStorage.LorenRuntime or next to this shim): run loren update\")\nreturn (require :: any)(rt)\n";

pub const LUAU_KEYWORDS: [&str; 21] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

pub fn is_keyword(name: &str) -> bool {
    LUAU_KEYWORDS.contains(&name)
}

/// `^[A-Za-z_][A-Za-z0-9_]*$`
pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Names Loren 2.0 reserves on a Service's Client table.
pub const RESERVED_CLIENT_NAMES: [&str; 5] =
    ["Server", "Signals", "ClientEvents", "Properties", "Try"];
/// Service keys that are new framework keys in 2.0.
pub const LEGACY_SERVICE_KEYS: [&str; 2] = ["Spec", "ClientEvents"];

pub const TYPES_FILE: &str = "LorenTypes.luau";
pub const SERVER_TYPES_FILE: &str = "LorenServerTypes.luau";
pub const BACKUP_DIR: &str = ".loren-backup";
pub const PROMISE_DIR: &str = "Promise";
/// What we keep of the vendored Promise. Spec files never ship.
pub const PROMISE_KEEP: [&str; 3] = ["lib", "default.project.json", "LICENSE"];

pub const SCAFFOLD_ENTRIES: [&str; 5] = [
    "default.project.json",
    "selene.toml",
    "loren",
    "loren_packages",
    "src",
];
/// Rojo and Argon skip these under loren_packages, so test code never reaches a place.
pub const GLOB_IGNORE_PATHS: [&str; 2] = [
    "loren_packages/**/*.spec.lua",
    "loren_packages/**/*.spec.luau",
];

/// `/\.spec\.luau?$/i`
pub fn is_spec_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".spec.lua") || lower.ends_with(".spec.luau")
}

pub const GITIGNORE_LINES: [&str; 6] = [
    "sourcemap.json",
    ".loren-backup/",
    "*.rbxl",
    "*.rbxlx",
    "*.rbxl.lock",
    "*.rbxlx.lock",
];

/// `loren inject <kind>`: (kind, premade folder, project folder key).
pub const PREMADE_KINDS: [(&str, &str, &str); 3] = [
    ("service", "services", "services"),
    ("controller", "controllers", "controllers"),
    ("shared", "shared", "shared"),
];
pub const PREMADE_DIR: &str = "loren_premade";
pub const RETIRED_PREMADES: [&str; 2] = [
    "services/ExampleService.luau",
    "controllers/ExampleController.luau",
];

/// `loren make`'s source: the scaffold's own examples (kind, file, placeholder).
pub const MODULE_TEMPLATES: [(&str, &str, &str); 2] = [
    (
        "service",
        "src/server/Services/ExampleService.luau",
        "ExampleService",
    ),
    (
        "controller",
        "src/client/Controllers/ExampleController.luau",
        "ExampleController",
    ),
];
