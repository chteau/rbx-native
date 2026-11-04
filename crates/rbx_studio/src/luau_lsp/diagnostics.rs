//! The reply to `workspace/diagnostic`, one report per file of the mirror,
//! read back as problems per script.
//!
//! Pulled rather than pushed: the server only pushes for documents a tab has
//! open, and Script Analysis is the whole place.

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use rbx_dom::{Ref, Variant, WeakDom};
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

/// Loren's duplicate-name warnings for every script of `dom`, in the same
/// shape as the server's problems (columns are bytes into the line, like the
/// editor's own conversion expects).
pub(crate) fn loren_problems(dom: &WeakDom) -> Vec<(Ref, Vec<Diagnostic>)> {
    let mut scripts: Vec<(Ref, Vec<Diagnostic>)> = Vec::new();
    for issue in rbx_loren::lint::duplicate_names(dom, &rbx_loren::lint::is_class_member) {
        let Some(Variant::String(source)) = dom
            .get(issue.script)
            .and_then(|script| script.properties().get("Source"))
        else {
            continue;
        };
        let position = |offset: usize| {
            let before = &source[..offset];
            let line_start = before.rfind('\n').map_or(0, |n| n + 1);
            Position::new(
                before.matches('\n').count() as u32,
                (offset - line_start) as u32,
            )
        };
        let problem = Diagnostic {
            range: Range::new(position(issue.range.start), position(issue.range.end)),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String(issue.kind.into())),
            source: Some("Loren".into()),
            message: issue.message,
            ..Diagnostic::default()
        };
        match scripts.last_mut() {
            Some((script, problems)) if *script == issue.script => problems.push(problem),
            _ => scripts.push((issue.script, vec![problem])),
        }
    }
    scripts
}

/// What the dock and the squiggles show: the server's problems and Loren's,
/// together, so their counts cannot disagree. Scripts in referent order, each
/// script's problems in line order.
pub(crate) fn merge(
    server: &[(Ref, Vec<Diagnostic>)],
    loren: &[(Ref, Vec<Diagnostic>)],
) -> Vec<(Ref, Vec<Diagnostic>)> {
    let mut scripts: Vec<(Ref, Vec<Diagnostic>)> = Vec::new();
    for (script, problems) in server.iter().chain(loren) {
        match scripts.iter_mut().find(|(s, _)| s == script) {
            Some((_, merged)) => merged.extend(problems.iter().cloned()),
            None => scripts.push((*script, problems.clone())),
        }
    }
    for (_, problems) in &mut scripts {
        problems.sort_by_key(|p| (p.range.start.line, p.range.start.character));
    }
    scripts.sort_by_key(|(script, _)| *script);
    scripts
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    /// Information and hints.
    pub(crate) notes: usize,
}

pub(crate) fn counts(scripts: &[(Ref, Vec<Diagnostic>)]) -> Counts {
    let mut counts = Counts::default();
    for problem in scripts.iter().flat_map(|(_, problems)| problems) {
        match problem.severity {
            Some(DiagnosticSeverity::ERROR) => counts.errors += 1,
            Some(DiagnosticSeverity::WARNING) => counts.warnings += 1,
            _ => counts.notes += 1,
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
                warnings: 1,
                notes: 0,
            }
        );
    }

    #[test]
    fn a_reply_without_items_is_no_problems() {
        let mirror = Mirror::new(std::env::temp_dir().join("rbx-luau-diag-none"));
        assert!(parse(&Value::Null, &mirror).is_empty());
    }

    #[test]
    fn loren_warnings_carry_their_line_and_column_and_merge_in_line_order() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        dom.new_instance("Part", "Part", Some(workspace));
        dom.new_instance("Part", "Part", Some(workspace));
        let script = dom.new_instance("Script", "Main", Some(workspace));
        let source = "print(1)\n  local a = workspace.Part\n";
        let _ = dom.set_property(script, "Source", Variant::String(source.into()));

        let loren = loren_problems(&dom);
        assert_eq!(loren.len(), 1);
        let warning = &loren[0].1[0];
        assert_eq!(warning.range.start, Position::new(1, 22));
        assert_eq!(warning.range.end, Position::new(1, 26));
        assert_eq!(warning.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(warning.source.as_deref(), Some("Loren"));
        assert_eq!(
            warning.code,
            Some(NumberOrString::String("duplicate-name".into()))
        );

        let server = vec![(
            script,
            vec![Diagnostic {
                range: Range::new(Position::new(3, 0), Position::new(3, 1)),
                severity: Some(DiagnosticSeverity::ERROR),
                message: "late".into(),
                ..Diagnostic::default()
            }],
        )];
        let merged = merge(&server, &loren);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].1.len(), 2);
        assert_eq!(merged[0].1[0].source.as_deref(), Some("Loren"));
        assert_eq!(
            counts(&merged),
            Counts {
                errors: 1,
                warnings: 1,
                notes: 0
            }
        );
        assert_eq!(merge(&[], &loren), loren);
    }
}
