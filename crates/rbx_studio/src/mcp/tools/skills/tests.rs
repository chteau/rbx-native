use serde_json::{json, Value};

use super::{call, parse, skills, FILES};

fn text(outcome: Result<Value, String>) -> (bool, String) {
    match outcome {
        Ok(Value::String(text)) => (false, text),
        Err(text) => (true, text),
        Ok(other) => panic!("skill answers text, got {other}"),
    }
}

#[test]
fn every_file_parses_with_a_unique_name_and_stays_short() {
    let parsed: Vec<_> = skills().collect();
    assert_eq!(
        parsed.len(),
        FILES.len(),
        "a skill file lacks a name or description"
    );
    let mut names: Vec<&str> = parsed.iter().map(|skill| skill.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), FILES.len(), "two skills share a name");
    for (file, skill) in FILES.iter().zip(&parsed) {
        assert!(
            file.lines().count() <= 400,
            "{} is over 400 lines",
            skill.name
        );
        assert!(
            !skill.body.starts_with("---"),
            "{}'s front matter leaked",
            skill.name
        );
        assert!(!file.contains('\u{2014}'), "{} has an em dash", skill.name);
    }
}

#[test]
fn no_name_lists_the_catalogue() {
    let (failed, listing) = text(call(&json!({})));
    assert!(!failed);
    assert!(listing.starts_with("Available skills:\n"));
    for skill in skills() {
        assert!(listing.contains(&format!("- {}: {}", skill.name, skill.description)));
    }
}

#[test]
fn a_name_returns_that_skill_ignoring_case() {
    let (failed, body) = text(call(&json!({ "name": "UI-Style" })));
    assert!(!failed);
    assert!(body.starts_with("# Skill: ui-style\n\n"));
    assert!(!body.contains("\ndescription:"));
}

#[test]
fn an_unknown_name_fails_with_the_catalogue() {
    let (failed, answer) = text(call(&json!({ "name": "nope" })));
    assert!(failed);
    assert!(answer.starts_with("Unknown skill: nope\n\nAvailable skills:"));
}

#[test]
fn the_listed_tool_names_every_skill_where_studio_lists_its_own() {
    let tools = crate::mcp::tools::list();
    let tool = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "skill")
        .unwrap();
    let description = tool["description"].as_str().unwrap();
    let options = tool["inputSchema"]["properties"]["skill_name"]["description"]
        .as_str()
        .unwrap();
    assert!(
        !description.contains("rbx-debug"),
        "Studio's own skills stay out"
    );
    assert!(description.ends_with("Only name a skill to the user after loading it."));
    for skill in skills() {
        assert!(description.contains(&format!("<name>{}</name>", skill.name)));
        assert!(options.contains(skill.name));
    }
}

#[test]
fn skill_name_is_studios_argument() {
    let (failed, body) = text(call(&json!({ "skill_name": "growth-analytics" })));
    assert!(!failed);
    assert!(body.starts_with("# Skill: growth-analytics\n\n"));
}

#[test]
fn front_matter_needs_both_fields() {
    assert!(parse("---\nname: a\n---\nbody").is_none());
    assert!(parse("no front matter").is_none());
    let skill = parse("---\nname: a\ndescription: \"b: c\"\n---\n\nbody").unwrap();
    assert_eq!(
        (skill.name, skill.description, skill.body),
        ("a", "b: c", "body")
    );
}

#[test]
fn a_crlf_checkout_parses_the_same() {
    // Built from LF so the test means the same on a CRLF checkout.
    let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
    let lf = leak(FILES[0].replace("\r\n", "\n"));
    let crlf = leak(lf.replace('\n', "\r\n"));
    let (lf, crlf) = (parse(lf).unwrap(), parse(crlf).unwrap());
    assert_eq!((lf.name, lf.description), (crlf.name, crlf.description));
    assert_eq!(lf.body.lines().next(), crlf.body.lines().next());
}

/// The building skills hand their Luau to `execute_luau` in Edit, so every
/// block has to run in the same runtime against a bare place.
#[test]
fn building_snippets_run_in_the_edit_runtime() {
    use rbx_dom::{Instance, Ref, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    for skill in skills().filter(|skill| skill.name.starts_with("build-")) {
        let blocks = skill.body.split("```lua").skip(1);
        for block in blocks.map(|rest| rest.split("```").next().unwrap_or_default()) {
            let mut dom = WeakDom::new();
            dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
            let mut runtime = rbx_lua::Runtime::new(dom, ReflectionDatabase::embedded()).unwrap();
            if let Err(err) = runtime.run(block) {
                panic!("{}: a Luau block fails: {err}\n{block}", skill.name);
            }
        }
    }
}
