//! The reply to `workspace/diagnostic`, one report per file of the mirror,
//! read back as problems per script.
//!
//! Pulled rather than pushed: the server only pushes for documents a tab has
//! open, and Script Analysis is the whole place.

use lsp_types::{Diagnostic, DiagnosticSeverity};
use rbx_dom::Ref;
use serde_json::Value;

use super::Mirror;

/// Every script with at least one problem, in referent order so the dock's
/// list holds still between refreshes, each script's problems in line order.
pub(crate) fn parse(reply: &Value, mirror: &Mirror) -> Vec<(Ref, Vec<Diagnostic>)> {
    let Some(items) = reply["items"].as_array() else {
        return Vec::new();
    };
    let mut scripts: Vec<(Ref, Vec<Diagnostic>)> = items
        .iter()
        .filter_map(|report| {
            let path = super::path(report["uri"].as_str()?)?;
            let script = mirror.script_at(&path)?;
            let mut problems: Vec<Diagnostic> =
                serde_json::from_value(report["items"].clone()).ok()?;
            problems
                .sort_by_key(|problem| (problem.range.start.line, problem.range.start.character));
            (!problems.is_empty()).then_some((script, problems))
        })
        .collect();
    scripts.sort_by_key(|(script, _)| *script);
    scripts
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
}

pub(crate) fn counts(scripts: &[(Ref, Vec<Diagnostic>)]) -> Counts {
    let mut counts = Counts::default();
    for problem in scripts.iter().flat_map(|(_, problems)| problems) {
        match problem.severity {
            Some(DiagnosticSeverity::ERROR) => counts.errors += 1,
            Some(DiagnosticSeverity::WARNING) => counts.warnings += 1,
            _ => {}
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Variant, WeakDom};
    use rbx_reflection::ReflectionDatabase;
    use serde_json::json;

    use super::*;

    #[test]
    fn keeps_scripts_with_problems_sorted_and_drops_everything_else() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let a = dom.new_instance("Script", "A", Some(workspace));
        let b = dom.new_instance("Script", "B", Some(workspace));
        let _ = dom.set_property(a, "Source", Variant::String(String::new()));
        let root = std::env::temp_dir().join(format!("rbx-luau-diag-{}", std::process::id()));
        let mut mirror = Mirror::new(root);
        mirror.sync(&dom, &ReflectionDatabase::embedded()).unwrap();

        let problem = |line: u32, severity: u32, message: &str| {
            json!({
                "range": {"start": {"line": line, "character": 0}, "end": {"line": line, "character": 1}},
                "severity": severity,
                "message": message,
            })
        };
        let report =
            |uri: String, items: Value| json!({"kind": "full", "uri": uri, "items": items});
        let reply = json!({"items": [
            report(super::super::uri(&mirror.path_of(b)), json!([problem(4, 2, "late"), problem(1, 1, "early")])),
            report(super::super::uri(&mirror.path_of(a)), json!([])),
            report("file:///defs.d.luau".into(), json!([problem(0, 1, "not a script")])),
        ]});

        let scripts = parse(&reply, &mirror);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].0, b);
        let messages: Vec<&str> = scripts[0].1.iter().map(|p| p.message.as_str()).collect();
        assert_eq!(messages, ["early", "late"]);
        assert_eq!(
            counts(&scripts),
            Counts {
                errors: 1,
                warnings: 1
            }
        );
    }

    #[test]
    fn a_reply_without_items_is_no_problems() {
        let mirror = Mirror::new(std::env::temp_dir().join("rbx-luau-diag-none"));
        assert!(parse(&Value::Null, &mirror).is_empty());
    }
}
