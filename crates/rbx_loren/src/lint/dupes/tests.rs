use rbx_dom::{Variant, WeakDom};

use super::*;

/// Workspace with two Parts named "Part", a Model with unique children, and a
/// Script in ServerScriptService holding `source`.
fn place(source: &str) -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let ws = dom.new_instance("Workspace", "Workspace", None);
    dom.new_instance("Part", "Part", Some(ws));
    dom.new_instance("Part", "Part", Some(ws));
    let model = dom.new_instance("Model", "Model", Some(ws));
    dom.new_instance("Part", "Door", Some(model));
    let sss = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let script = dom.new_instance("Script", "Main", Some(sss));
    dom.set_property(script, "Source", Variant::String(source.into()))
        .unwrap();
    (dom, script)
}

fn lint(source: &str) -> Vec<DupeIssue> {
    duplicate_names(&place(source).0, &is_class_member)
}

fn flagged(source: &str) -> Vec<String> {
    lint(source)
        .iter()
        .map(|i| source[i.range.clone()].to_string())
        .collect()
}

#[test]
fn indexing_a_shared_name_four_ways_is_flagged_each_time() {
    let src = "local a = workspace.Part\nlocal b = workspace[\"Part\"]\nlocal c = workspace:FindFirstChild('Part')\nlocal d = workspace:WaitForChild(\"Part\", 5)\n";
    let issues = lint(src);
    assert_eq!(
        issues.iter().map(|i| i.line).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert!(issues.iter().all(|i| &src[i.range.clone()] == "Part"));
    assert_eq!(issues[0].kind, "duplicate-name");
    assert_eq!(issues[0].severity, Severity::Warn);
    assert_eq!(
        issues[0].message,
        "workspace.Part: Workspace has 2 children named \"Part\", and which one this gets depends on child order (it changes when one is re-parented). Rename one so the name is unique."
    );
    assert!(issues[2]
        .message
        .starts_with("workspace:FindFirstChild('Part'): "));
}

#[test]
fn a_unique_chain_and_a_missing_name_are_quiet() {
    assert!(flagged("print(workspace.Model.Door, workspace.Nothing.Part)").is_empty());
}

#[test]
fn a_property_of_the_class_wins_over_a_child() {
    let (mut dom, script) =
        place("local s = workspace.Model.Door.Size\nlocal t = workspace.Part.Size");
    let door = dom.get(dom.root_refs()[0]).unwrap().children()[2];
    let door = dom.get(door).unwrap().children()[0];
    dom.new_instance("Part", "Size", Some(door));
    dom.new_instance("Part", "Size", Some(door));
    let issues = duplicate_names(&dom, &is_class_member);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].script, script);
    assert_eq!(issues[0].line, 2);
}

#[test]
fn a_recursive_find_first_child_searches_descendants_and_is_skipped() {
    assert!(flagged("workspace:FindFirstChild(\"Part\", true)").is_empty());
    assert_eq!(
        flagged("workspace:FindFirstChild(\"Part\", false)"),
        ["Part"]
    );
}

#[test]
fn an_alias_set_once_carries_the_chain_on() {
    let dom_with_dupes = |src: &str| {
        let (mut dom, _) = place(src);
        let model = dom.get(dom.root_refs()[0]).unwrap().children()[2];
        dom.new_instance("Part", "Twin", Some(model));
        dom.new_instance("Part", "Twin", Some(model));
        duplicate_names(&dom, &is_class_member)
    };
    assert_eq!(
        dom_with_dupes("local m = workspace.Model\nprint(m.Twin)").len(),
        1
    );
    assert!(dom_with_dupes("local m = workspace.Model\nm = nil\nprint(m.Twin)").is_empty());
}

#[test]
fn script_parent_and_get_service_start_chains() {
    let (mut dom, _) =
        place("print(script.Parent.Dup, game:GetService(\"Workspace\").Part, game.Workspace.Part)");
    let sss = dom.root_refs()[1];
    dom.new_instance("Folder", "Dup", Some(sss));
    dom.new_instance("Folder", "Dup", Some(sss));
    let issues = duplicate_names(&dom, &is_class_member);
    let messages: Vec<&str> = issues.iter().map(|i| i.message.as_str()).collect();
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].starts_with("script.Parent.Dup: ServerScriptService has 2"));
    assert!(messages[1].starts_with("game:GetService(\"Workspace\").Part: "));
    assert!(messages[2].starts_with("game.Workspace.Part: "));
}

#[test]
fn strings_comments_and_member_access_are_not_chains() {
    let src = "print(\"workspace.Part\")\n-- workspace.Part\n--[[ workspace.Part ]]\nlocal t = {}\nprint(t.workspace.Part, t:workspace().Part)";
    assert!(flagged(src).is_empty());
}
