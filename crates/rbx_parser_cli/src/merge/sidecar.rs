//! The `<path>.merge.json` file left beside a file that merged with
//! conflicts, read by the editor to offer resolving them.
//!
//! ```json
//! {
//!   "version": 1,
//!   "tool": "rbxmerge",
//!   "file": "<path as git gave it, relative to the work tree top>",
//!   "base": "<base64 (standard alphabet) of the base file's bytes>",
//!   "theirs": "<base64 (standard alphabet) of the theirs file's bytes>",
//!   "conflicts": [
//!     { "path": "Workspace.Part", "class": "Part", "target": "Transparency" }
//!   ]
//! }
//! ```
//!
//! `target` is `Name`, `ClassName`, the property name, `Attribute <name>`,
//! `Source hunk <n>` (1-based), `Parent`, `Removed by us` or `Removed by
//! them`. The editor re-merges `base`, the merged file as it stands and
//! `theirs`; the merged file holds ours wherever the sides conflicted, so
//! that merge reproduces exactly the conflicts recorded here.

use base64::Engine as _;
use rbx_dom::merge::{Conflict, Side, Target};

pub fn path_for(file: &str) -> String {
    format!("{file}.merge.json")
}

pub fn target_text(target: &Target) -> String {
    match target {
        Target::Name => "Name".to_string(),
        Target::Class => "ClassName".to_string(),
        Target::Property(name) => name.clone(),
        Target::Attribute(name) => format!("Attribute {name}"),
        Target::Source(index) => format!("Source hunk {}", index + 1),
        Target::Parent => "Parent".to_string(),
        Target::Existence {
            removed_by: Side::Ours,
        } => "Removed by us".to_string(),
        Target::Existence { .. } => "Removed by them".to_string(),
    }
}

pub fn write(file: &str, base: &[u8], theirs: &[u8], conflicts: &[Conflict]) -> Result<(), String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let entries: Vec<_> = conflicts
        .iter()
        .map(|c| {
            serde_json::json!({
                "path": c.path,
                "class": c.class,
                "target": target_text(&c.target),
            })
        })
        .collect();
    let document = serde_json::json!({
        "version": 1,
        "tool": "rbxmerge",
        "file": file,
        "base": engine.encode(base),
        "theirs": engine.encode(theirs),
        "conflicts": entries,
    });
    let text = serde_json::to_string_pretty(&document)
        .map_err(|err| format!("failed to encode merge sidecar: {err}"))?;
    let path = path_for(file);
    std::fs::write(&path, text).map_err(|err| format!("failed to write '{path}': {err}"))
}

/// A sidecar from an earlier merge of the file would describe conflicts that
/// no longer exist.
pub fn remove(file: &str) -> Result<(), String> {
    let path = path_for(file);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("failed to remove '{path}': {err}")),
    }
}
