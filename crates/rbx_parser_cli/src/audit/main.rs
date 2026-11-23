//! `rbxaudit`: runs the whole-place script security audit, or the full
//! pre-publish check, from the command line, for CI.
//!
//! ```text
//! rbxaudit [--prepublish [--offline]] [--json] <place.rbxl|.rbxlx|.rbxm|.rbxmx>
//! ```
//!
//! One line per finding on stdout (`severity check path:line message`, the
//! `:line` omitted for a place-level finding), then a summary line; with
//! `--json`, one JSON object instead (`findings` and `summary`).
//!
//! `--prepublish` adds the pre-publish rules (`docs/prepublish-check.md`):
//! it asks Roblox's asset service whether each asset id exists (read-only
//! GETs, no key) unless `--offline`, and measures meshes found in the local
//! asset cache.
//!
//! Exit codes: `0` no High finding, `1` at least one High finding, `2` bad
//! usage or an unreadable file (message on stderr).

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

use rbx_assets::{AssetCache, AssetRef};
use rbx_audit::prepublish::{self, Inputs, Presence};
use rbx_audit::{Finding, Severity};
use rbx_cloud::AssetPresence;
use rbx_dom::WeakDom;

const USAGE: &str =
    "usage: rbxaudit [--prepublish [--offline]] [--json] <place.rbxl|.rbxlx|.rbxm|.rbxmx>";

struct Options {
    prepublish: bool,
    offline: bool,
    json: bool,
    path: String,
}

fn options(args: &[String]) -> Option<Options> {
    let mut options = Options {
        prepublish: false,
        offline: false,
        json: false,
        path: String::new(),
    };
    for arg in args {
        match arg.as_str() {
            "--prepublish" => options.prepublish = true,
            "--offline" => options.offline = true,
            "--json" => options.json = true,
            flag if flag.starts_with('-') => return None,
            path if options.path.is_empty() => options.path = path.to_string(),
            _ => return None,
        }
    }
    (!options.path.is_empty() && (options.prepublish || !options.offline)).then_some(options)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [flag] if flag == "--help" || flag == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let Some(options) = options(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let dom = match read_dom(Path::new(&options.path)) {
        Ok(dom) => dom,
        Err(err) => {
            eprintln!("rbxaudit: {err}");
            return ExitCode::from(2);
        }
    };
    let findings = if options.prepublish {
        prepublish_check(&dom, options.offline)
    } else {
        rbx_audit::audit(&dom)
    };
    if options.json {
        println!("{}", json(&findings));
    } else {
        for finding in &findings {
            println!("{}", line(finding));
        }
        println!("{}", summary(&findings));
    }
    if findings.iter().any(|f| f.severity == Severity::High) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn prepublish_check(dom: &WeakDom, offline: bool) -> Vec<Finding> {
    let ids = prepublish::asset_ids(dom);
    let answers = if offline {
        Vec::new()
    } else {
        rbx_cloud::Client::new(None).asset_presences(&ids)
    };
    // No answer at all is offline: every id not checked.
    let presence: Option<HashMap<u64, Presence>> = answers.iter().any(Option::is_some).then(|| {
        ids.iter()
            .zip(answers)
            .filter_map(|(id, answer)| {
                let presence = match answer? {
                    AssetPresence::Exists => Presence::Exists,
                    AssetPresence::Missing => Presence::Missing,
                    AssetPresence::Restricted => Presence::Restricted,
                };
                Some((*id, presence))
            })
            .collect()
    });
    // Read only: the cache is opened when it's already there, never made.
    let cache = rbx_assets::cache_root()
        .map(|root| root.join("assets"))
        .filter(|dir| dir.is_dir())
        .and_then(|dir| AssetCache::new(Some(dir)).ok());
    let mut fetch = |asset: &AssetRef| match (asset, &cache) {
        (AssetRef::Id(id), Some(cache)) => cache.get_id(*id),
        (AssetRef::Native(path), Some(cache)) => cache.get_native(path),
        _ => None,
    };
    prepublish::check(
        dom,
        Inputs {
            presence: presence.as_ref(),
            fetch: &mut fetch,
        },
    )
}

fn line(finding: &Finding) -> String {
    let location = match finding.line {
        Some(line) => format!("{}:{line}", finding.path),
        None => finding.path.clone(),
    };
    format!(
        "{:<6} {} {location} {}",
        finding.severity.label(),
        finding.check.id(),
        finding.message
    )
}

fn counts(findings: &[Finding]) -> [usize; 3] {
    [Severity::High, Severity::Medium, Severity::Low]
        .map(|severity| findings.iter().filter(|f| f.severity == severity).count())
}

fn summary(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "no findings".to_string();
    }
    let [high, medium, low] = counts(findings);
    format!(
        "{} findings: {high} high, {medium} medium, {low} low",
        findings.len(),
    )
}

fn json(findings: &[Finding]) -> serde_json::Value {
    let [high, medium, low] = counts(findings);
    serde_json::json!({
        "findings": findings
            .iter()
            .map(|f| serde_json::json!({
                "severity": f.severity.label(),
                "check": f.check.id(),
                "title": f.check.title(),
                "path": f.path,
                "line": f.line,
                "message": f.message,
            }))
            .collect::<Vec<_>>(),
        "summary": { "high": high, "medium": medium, "low": low },
    })
}

/// Sniffs the format from the bytes, not the extension, like `rbxdump`.
fn read_dom(path: &Path) -> Result<WeakDom, String> {
    let display = path.display();
    let bytes = std::fs::read(path).map_err(|err| format!("failed to read '{display}': {err}"))?;

    if rbx_xml::is_xml(&bytes) {
        let text = std::str::from_utf8(&bytes)
            .map_err(|err| format!("'{display}' is not valid UTF-8 XML: {err}"))?;
        rbx_xml::deserialize(text).map_err(|err| format!("failed to parse '{display}': {err}"))
    } else {
        rbx_binary::deserialize(&bytes).map_err(|err| format!("failed to parse '{display}': {err}"))
    }
}
