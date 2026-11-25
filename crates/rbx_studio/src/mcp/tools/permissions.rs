//! Which capability each tool needs, checked against Settings ›
//! Permissions before the tool runs. As in Studio, a client is never
//! prompted: a refused call says what is missing and where to turn it on.

use rbx_permissions::Capability;
use serde_json::Value;

use super::{input, luau, text_argument, Session};

/// What `name` called with `arguments` needs. Tools that only describe
/// the editor, read Roblox's own documentation or catalogue, or refuse
/// anyway (the AI generators) need nothing.
pub(super) fn needs(name: &str, arguments: &Value) -> Vec<Capability> {
    use Capability::*;
    let has = |key: &str| text_argument(arguments, key).is_some_and(|v| !v.trim().is_empty());
    match name {
        "search_game_tree"
        | "inspect_instance"
        | "script_read"
        | "script_search"
        | "script_grep"
        | luau::CONSOLE_OUTPUT
        | luau::SCREEN_CAPTURE => vec![ReadPlace],
        "multi_edit" => vec![EditScripts],
        luau::EXECUTE_LUAU
        | luau::START_STOP_PLAY
        | input::NAVIGATION
        | input::KEYBOARD
        | input::MOUSE => vec![RunCode],
        luau::SAVE_PLACE if has("path") => vec![Files],
        luau::SAVE_PLACE => vec![EditPlace],
        "insert_asset" => vec![EditPlace],
        "import_model" if has("path") => vec![EditPlace, CloudUpload, Files],
        "import_model" => vec![EditPlace, CloudUpload],
        "upload_image" => vec![CloudUpload, Files],
        "store_image" => vec![Files],
        _ => Vec::new(),
    }
}

pub(super) fn check(name: &str, arguments: &Value, session: &Session) -> Result<(), String> {
    let Some(policy) = session.permissions else {
        return Ok(());
    };
    match needs(name, arguments)
        .into_iter()
        .find(|capability| !policy.mcp_allows(*capability))
    {
        Some(missing) => Err(format!(
            "Permission '{}' has not been granted to AI assistants. The user can turn it on in \
             Settings \u{203a} Permissions \u{203a} AI assistants (MCP); ask them to, or do \
             the task another way.",
            missing.label()
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use rbx_permissions::{Capability, Level, Policy};
    use rbx_reflection::ReflectionDatabase;
    use serde_json::{json, Value};

    use super::needs;
    use crate::mcp::tools::call;
    use crate::mcp::tools::tests::{place, studio};
    use crate::mcp::Session;
    use crate::properties::Properties;

    fn call_with(policy: &Policy, tool: &str, arguments: Value) -> (bool, String, bool) {
        let dom = place();
        let properties = Properties::new(ReflectionDatabase::embedded());
        let studio = studio();
        let mut session = Session::new(&dom, &properties, &studio);
        session.permissions = Some(policy);
        let result = call(
            &json!({ "name": tool, "arguments": arguments }),
            &mut session,
        )
        .unwrap();
        (
            result["isError"].as_bool().unwrap(),
            result["content"][0]["text"].as_str().unwrap().to_owned(),
            session.planned.is_some(),
        )
    }

    #[test]
    fn every_tool_that_reaches_the_place_or_beyond_needs_something() {
        let tools = crate::mcp::tools::list();
        let free = [
            "list_roblox_studios",
            "get_studio_state",
            "skill",
            "http_get",
            "search_asset",
            "wait_job_finished",
            "subagent",
            "generate_mesh",
            "generate_material",
            "generate_procedural_model",
            "segment_mesh",
            "generate_texture",
        ];
        for tool in tools.as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            assert_eq!(
                needs(name, &json!({})).is_empty(),
                free.contains(&name),
                "{name}"
            );
        }
        assert_eq!(
            needs("save_place", &json!({ "path": "/tmp/x.rbxl" })),
            vec![Capability::Files]
        );
        assert_eq!(needs("save_place", &json!({})), vec![Capability::EditPlace]);
    }

    #[test]
    fn a_refused_tool_says_what_is_missing_and_plans_nothing() {
        let mut policy = Policy::default();
        policy.set_mcp(Capability::EditScripts, false);
        let edit = json!({
            "file_path": "game.ServerScriptService.GameLoop",
            "edits": [{ "old_string": "rounds += 1", "new_string": "rounds += 2" }]
        });
        let (error, text, planned) = call_with(&policy, "multi_edit", edit.clone());
        assert!(error);
        assert!(
            text.contains("'Script injection' has not been granted"),
            "{text}"
        );
        assert!(!planned);

        policy.set_mcp(Capability::EditScripts, true);
        let (error, text, planned) = call_with(&policy, "multi_edit", edit);
        assert!(!error, "{text}");
        assert!(planned);
    }

    #[test]
    fn safe_still_reads_the_place() {
        let mut policy = Policy::default();
        policy.set_mcp_level(Level::Safe);
        let (error, text, _) = call_with(
            &policy,
            "script_read",
            json!({ "target_file": "game.ServerScriptService.GameLoop" }),
        );
        assert!(!error, "{text}");
        let (error, text, _) = call_with(&policy, "execute_luau", json!({ "code": "return 1" }));
        assert!(error);
        assert!(text.contains("Run code & playtest"));
    }
}
