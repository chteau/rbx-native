//! Telling an MCP client how to reach the editor: the command it launches
//! (the [`super::bridge`]), written the way each client takes it, and
//! Claude Desktop's config file merged in place ([`desktop`]).
//!
//! Every path and snippet is built from explicit parts (the OS, the home
//! folder) so the Windows and macOS forms are tested on any machine.

pub(crate) mod desktop;
pub(crate) mod probe;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::BRIDGE_FLAG;

/// The name every client lists the server under.
pub(crate) const SERVER_NAME: &str = "rbx-native";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Os {
    Linux,
    Windows,
    Mac,
}

impl Os {
    pub(crate) const CURRENT: Os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    };
}

/// What a client runs to reach the editor, and the environment it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Launch {
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    /// Variables the editor found its config folder through, which a client
    /// started from the desktop may not have; the bridge must look in the
    /// same folder.
    pub(crate) env: Vec<(String, String)>,
}

impl Launch {
    /// This copy's command: `flatpak run` inside a Flatpak, else the
    /// AppImage or executable it runs from.
    pub(crate) fn current() -> Launch {
        let exe = crate::updater::launchable().unwrap_or_else(|_| PathBuf::from("rbxstudio"));
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|dir| !dir.is_empty());
        Launch::of(&exe, crate::updater::sandboxed(), config_home)
    }

    pub(crate) fn of(exe: &Path, flatpak: bool, config_home: Option<String>) -> Launch {
        let (command, args) = if flatpak {
            let args = ["run", crate::APP_ID, BRIDGE_FLAG];
            ("flatpak".to_owned(), args.map(str::to_owned).to_vec())
        } else {
            let exe = exe.to_string_lossy().into_owned();
            (exe, vec![BRIDGE_FLAG.to_owned()])
        };
        let env = config_home
            .map(|dir| ("XDG_CONFIG_HOME".to_owned(), dir))
            .into_iter()
            .collect();
        Launch { command, args, env }
    }

    /// What Test connection runs: the command itself, except inside a
    /// Flatpak, where `flatpak` can't be reached and this executable is the
    /// same bridge.
    pub(crate) fn probe_target(&self) -> PathBuf {
        if self.command == "flatpak" {
            std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rbxstudio"))
        } else {
            PathBuf::from(&self.command)
        }
    }

    /// The `{ "command", "args", "env"? }` object most clients' JSON takes.
    pub(crate) fn entry(&self) -> Value {
        let mut entry = Map::new();
        entry.insert("command".into(), json!(self.command));
        entry.insert("args".into(), json!(self.args));
        if !self.env.is_empty() {
            let env: Map<String, Value> = self
                .env
                .iter()
                .map(|(key, value)| (key.clone(), json!(value)))
                .collect();
            entry.insert("env".into(), Value::Object(env));
        }
        Value::Object(entry)
    }
}

/// The clients the AI page has setup steps for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Client {
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
    VsCode,
    Other,
}

impl Client {
    pub(crate) const ALL: [Client; 5] = [
        Client::ClaudeCode,
        Client::ClaudeDesktop,
        Client::Cursor,
        Client::VsCode,
        Client::Other,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Cursor => "Cursor",
            Client::VsCode => "VS Code",
            Client::Other => "Other",
        }
    }

    /// What to paste, and where.
    pub(crate) fn setup(self, launch: &Launch, os: Os, home: &Path) -> Setup {
        match self {
            Client::ClaudeCode => Setup {
                how: "Run this once in a terminal. It adds the server for every project, \
                      and claude mcp list should then show it connected.",
                snippet: claude_code_command(launch, os),
                file: None,
            },
            Client::ClaudeDesktop => Setup {
                how: "Merge this into the config file below, or let Add to Claude Desktop do it, \
                      then quit and reopen Claude Desktop.",
                snippet: servers_json("mcpServers", launch.entry()),
                file: Some(desktop::config_path(os, &desktop::Dirs::current(home))),
            },
            Client::Cursor => Setup {
                how: "Merge this into Cursor's global MCP file (Settings \u{203a} MCP \u{203a} \
                      Add new global MCP server opens it).",
                snippet: servers_json("mcpServers", launch.entry()),
                file: Some(home.join(".cursor").join("mcp.json")),
            },
            Client::VsCode => Setup {
                how:
                    "Save this as .vscode/mcp.json in your project, or run MCP: Open User \
                      Configuration and merge it there. Copilot Chat lists the tools in Agent mode.",
                snippet: servers_json("servers", vscode_entry(launch)),
                file: None,
            },
            Client::Other => Setup {
                how: "Any MCP client that launches a stdio server: give it this command and \
                      these arguments.",
                snippet: generic(launch),
                file: None,
            },
        }
    }
}

/// One client's steps: a line saying what to do, the text to copy, and the
/// file it goes in when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Setup {
    pub(crate) how: &'static str,
    pub(crate) snippet: String,
    pub(crate) file: Option<PathBuf>,
}

fn servers_json(key: &str, entry: Value) -> String {
    let document = json!({ key: { SERVER_NAME: entry } });
    serde_json::to_string_pretty(&document).unwrap_or_default()
}

fn vscode_entry(launch: &Launch) -> Value {
    let mut entry = Map::new();
    entry.insert("type".into(), json!("stdio"));
    if let Value::Object(rest) = launch.entry() {
        entry.extend(rest);
    }
    Value::Object(entry)
}

fn claude_code_command(launch: &Launch, os: Os) -> String {
    let mut words = vec!["claude", "mcp", "add", "--scope", "user"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for (key, value) in &launch.env {
        words.push("-e".to_owned());
        words.push(quote(&format!("{key}={value}"), os));
    }
    words.push(SERVER_NAME.to_owned());
    words.push("--".to_owned());
    words.push(quote(&launch.command, os));
    words.extend(launch.args.iter().map(|arg| quote(arg, os)));
    words.join(" ")
}

fn generic(launch: &Launch) -> String {
    let mut lines = vec![
        "Transport: stdio".to_owned(),
        format!("Command:   {}", launch.command),
        format!("Arguments: {}", launch.args.join(" ")),
    ];
    for (key, value) in &launch.env {
        lines.push(format!("Env:       {key}={value}"));
    }
    lines.join("\n")
}

/// `word` as one shell argument: POSIX single quotes on Linux and macOS,
/// double quotes on Windows (a path holds no `"` there).
pub(crate) fn quote(word: &str, os: Os) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:=@%+,".contains(c));
    let plain = plain && !(os == Os::Windows && word.contains('%'));
    if plain {
        word.to_owned()
    } else if os == Os::Windows {
        format!("\"{word}\"")
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}
