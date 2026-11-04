//! Per-file issues (`lint.js` lines 719-925): colon-style middleware, the
//! Service-shape warnings, and the "Did you mean" suggestion.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::analyze::analyze_luau;
use super::colon::colon_issues;
use super::tokenize::{structure, tokenize};
use crate::constants::RESERVED_CLIENT_NAMES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warn,
    Fixable,
}

/// Replace `start..end` (byte offsets) with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub file: String,
    pub line: usize,
    /// `colon-middleware`, `reserved-name`, `orphan-middleware`, `orphan-spec`, `reserved-key` or `duplicate-name`.
    pub kind: &'static str,
    pub message: String,
    pub severity: Severity,
    /// A fixable issue the rewrite cannot make (the body uses `self`).
    pub manual: bool,
    pub edit: Option<Edit>,
}

static INIT_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(init|\.src)\.luau?$").unwrap());
static LUA_EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.luau?$").unwrap());
static SCRIPT_EXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(server|client|local|legacy|plugin)\.luau?$").unwrap());

/// Edits applied from the last to the first.
pub fn apply_edits(src: &str, edits: &[Edit]) -> String {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.start));
    let mut out = src.to_string();
    for e in sorted {
        out = format!("{}{}{}", &out[..e.start], e.text, &out[e.end..]);
    }
    out
}

/// Case-insensitive edit distance.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = dp[0];
        dp[0] = i;
        for j in 1..=b.len() {
            let tmp = dp[j];
            let same = a[i - 1].to_lowercase().eq(b[j - 1].to_lowercase());
            dp[j] = (dp[j] + 1)
                .min(dp[j - 1] + 1)
                .min(prev + usize::from(!same));
            prev = tmp;
        }
    }
    dp[b.len()]
}

/// `" Did you mean 'X'?"` or `""`.
pub fn suggestion(name: &str, candidates: &[String]) -> String {
    let mut best: Option<&String> = None;
    let mut best_d = usize::MAX;
    for c in candidates {
        let d = levenshtein(name, c);
        if d < best_d {
            best = Some(c);
            best_d = d;
        }
    }
    match best {
        Some(b) if best_d <= 2.max(name.chars().count() / 3) => format!(" Did you mean '{b}'?"),
        _ => String::new(),
    }
}

fn reserved_list() -> String {
    let mut names = RESERVED_CLIENT_NAMES.to_vec();
    names.sort_unstable();
    names.join(", ")
}

/// The Instance name Rojo and Argon give a module file (`X.luau`, `X/init.luau`, `X/.src.luau`).
pub fn module_name_of(file: &str) -> String {
    let path = Path::new(file);
    let base = path
        .file_name()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    if INIT_FILE.is_match(&base) {
        return path
            .parent()
            .and_then(Path::file_name)
            .map(|b| b.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    LUA_EXT.replace(&base, "").into_owned()
}

/// `.server`/`.client`/`.local`/`.legacy`/`.plugin` scripts.
pub fn is_script_file(file: &str) -> bool {
    SCRIPT_EXT.is_match(file)
}

fn unique(lists: &[&[String]]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in lists.iter().flat_map(|l| l.iter()) {
        if !out.contains(n) {
            out.push(n.clone());
        }
    }
    out
}

/// All issues in one file, sorted by line; `service` enables the Service-shape checks.
pub fn scan_text(text: &str, file: &str, service: bool, legacy: bool) -> Vec<Issue> {
    let toks = tokenize(text);
    let s = structure(&toks);
    let name = module_name_of(file);
    let mut issues = colon_issues(text, &toks, &s, &name);
    for i in &mut issues {
        i.file = file.to_string();
    }
    if !service {
        return issues;
    }

    let info = analyze_luau(text, Some(&toks));
    let reserved = |n: &str| RESERVED_CLIENT_NAMES.contains(&n);
    let mut warn = |kind: &'static str, line: usize, message: String| {
        issues.push(Issue {
            file: file.to_string(),
            line,
            kind,
            message,
            severity: Severity::Warn,
            manual: false,
            edit: None,
        });
    };

    let list = reserved_list();
    for m in &info.client.methods {
        if reserved(&m.name) {
            warn(
                "reserved-name",
                m.line,
                format!("{name}.Client.{} uses a reserved name and is not networked (reserved: {list}). Rename it.", m.name),
            );
        }
    }
    let methods: Vec<String> = info
        .client
        .methods
        .iter()
        .filter(|m| !reserved(&m.name))
        .map(|m| m.name.clone())
        .collect();
    let events: Vec<String> = info.events.names.iter().map(|e| e.name.clone()).collect();
    let signals: Vec<String> = info.signals.names.iter().map(|e| e.name.clone()).collect();
    if info.client.known && info.events.known {
        let candidates = unique(&[&methods, &events]);
        for m in &info.middleware {
            if !candidates.contains(&m.name) {
                warn(
                    "orphan-middleware",
                    m.line,
                    format!(
                        "{name}.Middleware.{} matches no Client method or ClientEvent and is ignored.{}",
                        m.name,
                        suggestion(&m.name, &candidates)
                    ),
                );
            }
        }
    }
    if info.client.known && info.events.known && info.signals.known {
        let candidates = unique(&[&methods, &events, &signals]);
        for sp in &info.spec {
            if !candidates.contains(&sp.name) {
                warn(
                    "orphan-spec",
                    sp.line,
                    format!(
                        "{name}.Spec.{} matches no Client method, ClientEvent or Signal and is ignored.{}",
                        sp.name,
                        suggestion(&sp.name, &candidates)
                    ),
                );
            }
        }
    }
    for k in &info.service_keys {
        if k.empty || (!legacy && (k.modern || k.unknown)) {
            continue;
        }
        let meaning = if k.key == "Spec" {
            "per-member network specs (Loren.Method, Loren.Event, Loren.Signal)"
        } else {
            "the names of client-to-server events"
        };
        warn(
            "reserved-key",
            k.line,
            format!("{name}.{} is a Loren 2.0 key: Loren reads it as {meaning}. If it is your own data, rename it.", k.key),
        );
    }
    issues.sort_by_key(|i| i.line);
    issues
}
