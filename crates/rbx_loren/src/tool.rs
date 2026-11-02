//! The project's sync tool (`lib/tool.js`): which one it uses, its
//! sourcemap, its serve command.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::constants::{tool_pin, LOREN_CONFIG_FILE, MANAGERS, PROJECT_FILE};
use crate::ctx::Tool;
use crate::fsutil::read_text_if_exists;
use crate::run::{RunOpts, Runner, SpawnError, Stdio};
use crate::scriptsync::is_script_sync;

pub const SOURCEMAP_FILE: &str = "sourcemap.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInfo {
    pub tool: Option<Tool>,
    /// The manifest file name, `.loren.json` or `installed`.
    pub source: Option<String>,
}

/// A manifest entry names the tool by its GitHub repo, whatever the alias on the left is.
static ROJO_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?im)["'\s=]rojo-rbx/rojo(?:@|["'\s]|$)"#).expect("valid regex")
});
static ARGON_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?im)["'\s=]argon-rbx/argon(?:@|["'\s]|$)"#).expect("valid regex")
});
static ANSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid regex"));

/// Programs a manifest names by GitHub repo, comments ignored (`MANIFEST_PATTERNS`).
pub fn manifest_tools(text: &str) -> Vec<Tool> {
    let clean = text
        .split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.find('#').map_or(line, |i| &line[..i])
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut found = Vec::new();
    if ROJO_PATTERN.is_match(&clean) {
        found.push(Tool::Rojo);
    }
    if ARGON_PATTERN.is_match(&clean) {
        found.push(Tool::Argon);
    }
    found
}

pub fn is_installed(tool: Tool, run: &dyn Runner, cwd: &Path) -> bool {
    run.run(
        tool.id(),
        &["--version".to_string()],
        &RunOpts {
            cwd: Some(cwd.to_path_buf()),
            stdio: Stdio::Pipe,
        },
    )
    .ok()
}

pub fn detect_tool_info(root: &Path, run: &dyn Runner) -> ToolInfo {
    if is_script_sync(root) {
        return ToolInfo {
            tool: Some(Tool::None),
            source: Some(LOREN_CONFIG_FILE.to_string()),
        };
    }
    for (_, file) in MANAGERS {
        let Some(text) = read_text_if_exists(&root.join(file)) else {
            continue;
        };
        let found = manifest_tools(&text);
        // Both listed (Argon to sync, Rojo to build is common): Argon wins, as it did in 1.5.1.
        if found.contains(&Tool::Argon) {
            return ToolInfo {
                tool: Some(Tool::Argon),
                source: Some(file.to_string()),
            };
        }
        if found.contains(&Tool::Rojo) {
            return ToolInfo {
                tool: Some(Tool::Rojo),
                source: Some(file.to_string()),
            };
        }
    }
    // No manifest names one: whichever is installed, Rojo first (the documented default).
    for tool in Tool::PROGRAMS {
        if is_installed(tool, run, root) {
            return ToolInfo {
                tool: Some(tool),
                source: Some("installed".to_string()),
            };
        }
    }
    ToolInfo {
        tool: None,
        source: None,
    }
}

pub fn detect_tool(root: &Path, run: &dyn Runner) -> Option<Tool> {
    detect_tool_info(root, run).tool
}

/// `--tool` when given (validated), else what the project uses.
pub fn resolve_tool(
    root: &Path,
    tool_override: Option<&str>,
    run: &dyn Runner,
) -> Result<Tool, String> {
    if let Some(value) = tool_override.filter(|v| !v.is_empty()) {
        let Some(tool) = Tool::parse(value) else {
            return Err(format!(
                "Unknown sync tool \"{value}\". Use --tool rojo or --tool argon (or --tool none for Roblox Script Sync)."
            ));
        };
        if tool != Tool::None && is_script_sync(root) {
            return Err(format!(
                "This project uses Roblox Script Sync ({LOREN_CONFIG_FILE} says \"tool\": \"none\"), so --tool {tool} does not apply. Switching is not automated yet."
            ));
        }
        return Ok(tool);
    }
    if let Some(found) = detect_tool(root, run) {
        return Ok(found);
    }
    Err(format!(
        "No sync tool found. Add rojo-rbx/rojo or argon-rbx/argon to rokit.toml (or aftman.toml / foreman.toml), install one (for example: rokit add {}), or pass --tool rojo|argon|none.",
        tool_pin("rojo").map_or("rojo-rbx/rojo", |p| p.repo)
    ))
}

/// Whether this process runs inside a Flatpak sandbox.
fn in_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

pub fn sourcemap_args() -> Vec<String> {
    ["sourcemap", PROJECT_FILE, "-o", SOURCEMAP_FILE]
        .map(String::from)
        .to_vec()
}

// Argon writes its own sourcemap while serving (--sourcemap); Luau-LSP autogenerates it only for Rojo.
pub fn serve_args(tool: Tool) -> Result<Vec<String>, String> {
    match tool {
        Tool::None => {
            Err("Roblox Script Sync runs inside Studio: there is nothing to serve.".to_string())
        }
        Tool::Argon => Ok(["serve", PROJECT_FILE, "--sourcemap"]
            .map(String::from)
            .to_vec()),
        Tool::Rojo => Ok(["serve", PROJECT_FILE].map(String::from).to_vec()),
    }
}

/// Runs `<tool> sourcemap default.project.json -o sourcemap.json` in `root`.
pub fn sourcemap(root: &Path, tool: Tool, run: &dyn Runner) -> Result<(), String> {
    if tool == Tool::None {
        return Ok(());
    }
    let label = tool.label();
    let result = run.run(
        tool.id(),
        &sourcemap_args(),
        &RunOpts {
            cwd: Some(root.to_path_buf()),
            stdio: Stdio::Pipe,
        },
    );
    match &result.error {
        Some(SpawnError::NotFound) => {
            let repo = tool_pin(tool.id()).map_or("", |p| p.repo);
            let note = if in_flatpak() {
                " The Flatpak build of rbx-native can't see programs installed on the host."
            } else {
                ""
            };
            return Err(format!("{tool} is not installed or not on PATH. Install it (rokit add {repo}) or pass --tool.{note}"));
        }
        Some(SpawnError::Other(message)) => {
            return Err(format!("{label} could not start: {message}"))
        }
        None => {}
    }
    if result.status != Some(0) {
        let raw = if result.stderr.is_empty() {
            &result.stdout
        } else {
            &result.stderr
        };
        let stripped = ANSI.replace_all(raw, "");
        let lines: Vec<&str> = stripped.trim().lines().collect();
        let detail = lines[lines.len().saturating_sub(3)..].join(" ");
        let status = result.status.map_or("null".to_string(), |s| s.to_string());
        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        };
        return Err(format!("{label} sourcemap failed (exit {status}){suffix}"));
    }
    Ok(())
}
