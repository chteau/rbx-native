//! `loren doctor`'s lint pass (`lint.js` lines 926-1127: `projectLayout`, `runDoctor`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rbx_dom::WeakDom;

use super::dupes::{duplicate_names, is_class_member};
use super::scan::{apply_edits, is_script_file, scan_text, Edit, Issue, Severity};
use crate::constants::{Key, BACKUP_DIR, PREFIX, PROJECT_FILE};
use crate::ctx::{DoctorOptions, DoctorResult};
use crate::fsutil;
use crate::json::Json;
use crate::place;
use crate::project::node_path;
use crate::scriptsync::is_script_sync;

/// Project-relative folders (posix); the runtime ones only for Script Sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub shared: String,
    pub server: String,
    pub client: String,
    pub packages: String,
    pub runtime_shared: Option<String>,
    pub runtime_server: Option<String>,
}

fn child<'a>(node: Option<&'a Json>, keys: &[&str]) -> Option<&'a Json> {
    let mut n = node?;
    for key in keys {
        if !n.is_object() {
            return None;
        }
        n = n.get(key)?;
    }
    Some(n)
}

/// `data` when the caller already parsed `default.project.json`; `None` reads it.
pub fn project_layout(root: &Path, data: Option<&Json>) -> Layout {
    if is_script_sync(root) {
        return Layout {
            shared: Key::Shared.script_sync_path().to_string(),
            server: Key::Server.script_sync_path().to_string(),
            client: Key::Client.script_sync_path().to_string(),
            packages: Key::Packages.script_sync_path().to_string(),
            runtime_shared: Some(Key::RuntimeShared.script_sync_path().to_string()),
            runtime_server: Some(Key::RuntimeServer.script_sync_path().to_string()),
        };
    }
    let read;
    let data = match data {
        Some(d) => Some(d),
        None => {
            read = fsutil::read_text(&root.join(PROJECT_FILE))
                .ok()
                .and_then(|text| Json::parse(fsutil::strip_bom(&text)).ok());
            read.as_ref()
        }
    };
    let tree = child(data, &["tree"]);
    let pick = |keys: &[&str], fallback: &str| -> String {
        let Some(p) = child(tree, keys)
            .and_then(node_path)
            .filter(|p| !p.is_empty())
        else {
            return fallback.to_string();
        };
        let abs = fsutil::resolve(root, p);
        if fsutil::is_inside(root, &abs) {
            fsutil::rel(root, &abs)
        } else {
            fallback.to_string()
        }
    };
    Layout {
        shared: pick(&["ReplicatedStorage", "Shared"], Key::Shared.default_path()),
        server: pick(
            &["ServerScriptService", "Server"],
            Key::Server.default_path(),
        ),
        client: pick(
            &["StarterPlayer", "StarterPlayerScripts", "Client"],
            Key::Client.default_path(),
        ),
        packages: pick(
            &["ReplicatedStorage", "LorenPackages"],
            Key::Packages.default_path(),
        ),
        runtime_shared: None,
        runtime_server: None,
    }
}

/// Copies `file` (absolute, inside root) to `.loren-backup/<stamp>/<relative path>`. Keeps an older copy.
fn backup_file(root: &Path, file: &Path, stamp: &str) -> std::io::Result<PathBuf> {
    let target = root
        .join(BACKUP_DIR)
        .join(stamp)
        .join(fsutil::relative(root, file));
    if !fsutil::exists(&target) {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(file, &target)?;
    }
    Ok(target)
}

fn source_files(root: &Path, dirs: &[String], skip: &[String]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let skipped: Vec<PathBuf> = skip.iter().map(|d| root.join(d)).collect();
    for d in dirs {
        for f in fsutil::walk_files(&root.join(d), &fsutil::DEFAULT_SKIP) {
            if skipped.iter().any(|s| fsutil::is_inside(s, &f)) {
                continue;
            }
            let lower = f.to_string_lossy().to_ascii_lowercase();
            let is_lua = lower.ends_with(".lua") || lower.ends_with(".luau");
            if is_lua && fsutil::is_inside(root, &f) && seen.insert(f.clone()) {
                out.push(f);
            }
        }
    }
    out
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The place the project builds, linted for scripts that index a name two
/// siblings share. Nothing when the project has no tree to build.
fn duplicate_name_issues(root: &Path, skip_dirs: &[String]) -> Vec<Issue> {
    if !fsutil::is_file(&root.join(PROJECT_FILE)) && !is_script_sync(root) {
        return Vec::new();
    }
    let mut dom = WeakDom::new();
    let Ok(files) = place::apply_project_files(&mut dom, root) else {
        return Vec::new();
    };
    let skipped: Vec<PathBuf> = skip_dirs.iter().map(|d| root.join(d)).collect();
    duplicate_names(&dom, &is_class_member)
        .into_iter()
        .filter_map(|d| {
            let (_, path) = files.iter().find(|(script, _)| *script == d.script)?;
            if skipped.iter().any(|s| fsutil::is_inside(s, path)) {
                return None;
            }
            Some(Issue {
                file: fsutil::rel(root, path),
                line: d.line,
                kind: d.kind,
                message: d.message,
                severity: Severity::Warn,
                manual: false,
                edit: None,
            })
        })
        .collect()
}

pub fn run_doctor(root: &Path, opts: &DoctorOptions) -> DoctorResult {
    let log = opts.log;
    let layout = if opts.src_dir.is_some() && opts.server_dir.is_some() {
        None
    } else {
        Some(project_layout(root, None))
    };
    let server_dir = match (&opts.server_dir, &layout) {
        (Some(s), _) => s.clone(),
        (None, Some(l)) => l.server.clone(),
        (None, None) => String::new(),
    };
    let scan_dirs: Vec<String> = match (&opts.src_dir, &layout) {
        (Some(s), _) => vec![s.clone()],
        (None, Some(l)) => vec![
            "src".to_string(),
            l.shared.clone(),
            l.server.clone(),
            l.client.clone(),
        ],
        (None, None) => Vec::new(),
    };
    // Script Sync keeps the runtime inside the scanned folders; it is never linted or rewritten.
    let skip_dirs: Vec<String> = layout
        .iter()
        .flat_map(|l| [l.runtime_shared.clone(), l.runtime_server.clone()])
        .flatten()
        .collect();
    let server_root = root.join(&server_dir);
    let mut texts: Vec<(String, PathBuf, String)> = Vec::new();
    let mut issues: Vec<Issue> = Vec::new();
    for abs in source_files(root, &scan_dirs, &skip_dirs) {
        let text = match fsutil::read_text(&abs) {
            Ok(t) => t,
            Err(err) => {
                log.warn(&format!(
                    "Could not read {}: {err}",
                    fsutil::rel(root, &abs)
                ));
                continue;
            }
        };
        let file = fsutil::rel(root, &abs);
        let service =
            fsutil::is_inside(&server_root, &abs) && !is_script_file(&fsutil::to_posix(&abs));
        let found = scan_text(&text, &file, service, opts.legacy);
        if !found.is_empty() {
            texts.push((file, abs, text));
        }
        issues.extend(found);
    }

    issues.extend(duplicate_name_issues(root, &skip_dirs));

    if issues.is_empty() {
        log.ok("Middleware lint: no issues.");
        return DoctorResult {
            issues,
            fixed: Vec::new(),
            remaining: Vec::new(),
            exit_code: 0,
        };
    }

    log.info(&format!(
        "Middleware lint: {} issue{}.",
        issues.len(),
        plural(issues.len())
    ));
    for i in &issues {
        let tag = if i.severity == Severity::Fixable {
            if i.manual {
                log.red("[fix by hand]")
            } else {
                log.yellow("[fixable]")
            }
        } else {
            log.dim("[warning]")
        };
        log.plain(&format!("  {}:{} {} {}", i.file, i.line, tag, i.message));
    }

    let auto: Vec<usize> = (0..issues.len())
        .filter(|&n| issues[n].severity == Severity::Fixable && !issues[n].manual)
        .collect();
    let mut fixed_idx: Vec<usize> = Vec::new();
    let mut exit_code = 0;
    if opts.fix && !auto.is_empty() {
        let count = format!("{} middleware function{}", auto.len(), plural(auto.len()));
        let mut go = opts.yes;
        if !go && !opts.is_tty {
            log.error(&format!(
                "Rewriting {count} to dot style needs confirmation. Run again with --yes (the originals are backed up to {BACKUP_DIR}/)."
            ));
            exit_code = 2;
        } else if !go {
            let question =
                format!("{PREFIX} Rewrite {count} to dot style? The originals are backed up to {BACKUP_DIR}/.");
            go = opts.prompt.is_some_and(|p| p.confirm(&question, false));
        }
        if go {
            let stamp = fsutil::timestamp(opts.now.unwrap_or_else(SystemTime::now));
            let mut by_file: Vec<(&str, Vec<usize>)> = Vec::new();
            for &n in &auto {
                let file = issues[n].file.as_str();
                match by_file.iter_mut().find(|(f, _)| *f == file) {
                    Some((_, list)) => list.push(n),
                    None => by_file.push((file, vec![n])),
                }
            }
            for (file, list) in by_file {
                let Some((_, abs, text)) = texts.iter().find(|(f, _, _)| f == file) else {
                    continue;
                };
                let edits: Vec<Edit> = list
                    .iter()
                    .filter_map(|&n| issues[n].edit.clone())
                    .collect();
                let result = backup_file(root, abs, &stamp)
                    .and_then(|_| fsutil::write_text(abs, apply_edits(text, &edits)));
                match result {
                    Ok(()) => fixed_idx.extend(list),
                    Err(err) => log.error(&format!("Could not rewrite {file}: {err}")),
                }
            }
            if !fixed_idx.is_empty() {
                log.ok(&format!(
                    "Rewrote {} middleware function{} to dot style (backup: {BACKUP_DIR}/{stamp}/).",
                    fixed_idx.len(),
                    plural(fixed_idx.len())
                ));
            }
        } else if exit_code == 0 {
            log.info("Left the middleware as it is.");
        }
    }

    let fixed: Vec<Issue> = fixed_idx.iter().map(|&n| issues[n].clone()).collect();
    let remaining: Vec<Issue> = issues
        .iter()
        .enumerate()
        .filter(|(n, _)| !fixed_idx.contains(n))
        .map(|(_, i)| i.clone())
        .collect();
    if exit_code == 0 && remaining.iter().any(|i| i.severity == Severity::Fixable) {
        exit_code = 1;
        // --fix only helps when something is auto-fixable; [fix by hand] ones need the user.
        if !opts.fix
            && remaining
                .iter()
                .any(|i| i.severity == Severity::Fixable && !i.manual)
        {
            log.info("Run `loren doctor --fix` to rewrite colon-style middleware to dot style.");
        }
        if remaining.iter().any(|i| i.manual) {
            log.info("Rewrite the [fix by hand] middleware to dot style yourself, then run `loren doctor` again.");
        }
    }
    DoctorResult {
        issues,
        fixed,
        remaining,
        exit_code,
    }
}
