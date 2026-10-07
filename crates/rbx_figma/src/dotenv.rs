//! The gitignored `.env` at the checkout's root, read by `build.rs` for the
//! client secret and, in debug builds, at run time for a dev token. Shared
//! with `build.rs` through `#[path]`, so it uses std alone.

use std::path::{Path, PathBuf};

/// The first `.env` in `dir` or any directory above it.
pub fn find(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .map(|d| d.join(".env"))
        .find(|path| path.is_file())
}

/// `key`'s value in a `.env`: `KEY=VALUE` lines (an optional `export `
/// prefix), comments and blank lines skipped, one pair of matching quotes
/// stripped.
pub fn value_of(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (k, v) = line.split_once('=')?;
        if line.starts_with('#') || k.trim() != key {
            return None;
        }
        let v = v.trim();
        let unquoted = ['"', '\'']
            .iter()
            .find_map(|q| v.strip_prefix(*q)?.strip_suffix(*q))
            .unwrap_or(v);
        Some(unquoted.to_string()).filter(|v| !v.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_with_comments_quotes_and_export() {
        let text = "# RBX_FIGMA_TOKEN=commented\n\nOTHER=1\nexport RBX_FIGMA_TOKEN = \"figd_x\"\nQ='a b'\nE=\n";
        assert_eq!(value_of(text, "RBX_FIGMA_TOKEN").as_deref(), Some("figd_x"));
        assert_eq!(value_of(text, "Q").as_deref(), Some("a b"));
        assert_eq!(value_of(text, "OTHER").as_deref(), Some("1"));
        assert_eq!(value_of(text, "E"), None);
        assert_eq!(value_of(text, "MISSING"), None);
    }
}
