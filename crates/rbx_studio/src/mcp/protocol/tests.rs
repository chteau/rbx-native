use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Value};

use super::{handle, job_reply, Reply};
use crate::mcp::{image_content, text_content, Job, PlayState, Session, Studio};
use crate::properties::Properties;

fn reply(line: &str) -> Option<Value> {
    let dom = WeakDom::new();
    let properties = Properties::new(ReflectionDatabase::embedded());
    let studio = Studio {
        id: "a1".to_owned(),
        name: "Arena".to_owned(),
        place_id: Some(42),
        play: PlayState::Edit,
    };
    let mut session = Session::new(&dom, &properties, &studio);
    handle(line, &mut session).map(|reply| serde_json::from_str(&reply.wait()).unwrap())
}

#[test]
fn initialize_answers_the_version_asked_for_when_it_is_known() {
    let answer = reply(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
    )
    .unwrap();
    assert_eq!(answer["id"], 1);
    assert_eq!(answer["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(answer["result"]["serverInfo"]["name"], "rbx-native");
    assert!(answer["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn initialize_offers_the_newest_version_for_one_it_does_not_know() {
    let answer = reply(
        r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
    )
    .unwrap();
    assert_eq!(answer["id"], "a");
    assert_eq!(answer["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn notifications_and_client_responses_get_no_reply() {
    assert!(reply(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    assert!(reply(r#"{"jsonrpc":"2.0","id":4,"result":{}}"#).is_none());
}

#[test]
fn ping_is_an_empty_result() {
    let answer = reply(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#).unwrap();
    assert_eq!(answer["result"], json!({}));
}

/// A tool as the comparison sees it: its name, required arguments and
/// argument names, each sorted.
fn shape(tool: &Value) -> (String, Vec<String>, Vec<String>) {
    let schema = &tool["inputSchema"];
    let mut required: Vec<String> = schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str().map(str::to_owned))
        .collect();
    let mut properties: Vec<String> = schema["properties"]
        .as_object()
        .into_iter()
        .flat_map(|properties| properties.keys().cloned())
        .collect();
    required.sort();
    properties.sort();
    (
        tool["name"].as_str().unwrap().to_owned(),
        required,
        properties,
    )
}

/// Studio's own `tools/list`: a fresh dump from `RBX_MCP_STUDIO_TOOLS_FIXTURE`
/// when set (a first line starting with `#` is skipped), else the list this
/// editor serves, which mirrors Studio's names and arguments.
fn studio_tools() -> Vec<Value> {
    let text = match std::env::var_os("RBX_MCP_STUDIO_TOOLS_FIXTURE") {
        Some(path) => std::fs::read_to_string(path).unwrap(),
        None => include_str!("../tools/studio_tools.json").to_owned(),
    };
    let body = match text.strip_prefix('#') {
        Some(rest) => rest.split_once('\n').map_or("", |(_, body)| body),
        None => &text,
    };
    let parsed: Value = serde_json::from_str(body).unwrap();
    parsed
        .get("tools")
        .unwrap_or(&parsed)
        .as_array()
        .unwrap()
        .clone()
}

/// Our list mirrors Studio's names and arguments; the descriptions are our
/// own, so they are not compared, and our extra tools are left out.
#[test]
fn tools_list_matches_studios_names_and_arguments() {
    let answer = reply(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#).unwrap();
    let ours = answer["result"]["tools"].as_array().unwrap();
    let extras = crate::mcp::tools::extra_names();
    let mut listed: Vec<_> = ours
        .iter()
        .filter(|tool| !extras.iter().any(|name| tool["name"] == name.as_str()))
        .map(shape)
        .collect();
    let mut studio: Vec<_> = studio_tools().iter().map(shape).collect();
    listed.sort();
    studio.sort();
    assert_eq!(listed, studio);
    for tool in ours {
        if tool["name"] != "list_roblox_studios" {
            assert_eq!(
                tool["annotations"]["title"], tool["title"],
                "{}",
                tool["name"]
            );
        }
    }
}

#[test]
fn a_missing_studio_id_is_still_accepted_with_one_editor() {
    let answer = reply(
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_studio_state","arguments":{}}}"#,
    )
    .unwrap();
    assert_eq!(answer["result"]["isError"], false);
}

#[test]
fn malformed_and_unknown_messages_are_json_rpc_errors() {
    let parse = reply("{not json").unwrap();
    assert_eq!(parse["error"]["code"], -32700);
    assert_eq!(parse["id"], Value::Null);

    let batch = reply("[]").unwrap();
    assert_eq!(batch["error"]["code"], -32600);

    let unknown = reply(r#"{"jsonrpc":"2.0","id":5,"method":"resources/list"}"#).unwrap();
    assert_eq!(unknown["error"]["code"], -32601);
    assert_eq!(unknown["id"], 5);

    let tool = reply(
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"teleport_player"}}"#,
    )
    .unwrap();
    assert_eq!(tool["error"]["code"], -32602);
}

#[test]
fn a_shell_tool_is_handed_back_as_a_job() {
    let dom = WeakDom::new();
    let properties = Properties::new(ReflectionDatabase::embedded());
    let studio = Studio {
        id: "a1".to_owned(),
        name: "Arena".to_owned(),
        place_id: None,
        play: PlayState::Edit,
    };
    let mut session = Session::new(&dom, &properties, &studio);
    let line = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_console_output","arguments":{}}}"#;
    match handle(line, &mut session) {
        Some(Reply::Job { id, job }) => {
            assert_eq!(id, json!(9));
            assert_eq!(job, Job::Console);
        }
        _ => panic!("expected a job"),
    }
    assert!(session.job.is_none());
}

#[test]
fn job_reply_answers_text_images_and_errors() {
    let text: Value =
        serde_json::from_str(&job_reply(json!(1), Ok(vec![text_content("Game Started")]))).unwrap();
    assert_eq!(text["id"], 1);
    assert_eq!(text["result"]["isError"], false);
    assert_eq!(text["result"]["content"][0]["text"], "Game Started");

    let image: Value =
        serde_json::from_str(&job_reply(json!(2), Ok(vec![image_content(b"jpg")]))).unwrap();
    assert_eq!(
        image["result"]["content"][0],
        json!({ "type": "image", "data": "anBn", "mimeType": "image/jpeg" })
    );

    let failed: Value = serde_json::from_str(&job_reply(json!(3), Err("boom".to_owned()))).unwrap();
    assert_eq!(failed["result"]["isError"], true);
    assert_eq!(failed["result"]["content"][0]["text"], "boom");
}

#[test]
fn a_tool_that_fails_says_so_in_its_result() {
    let answer = reply(
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"inspect_instance","arguments":{"path":"game.Workspace.Nothing"}}}"#,
    )
    .unwrap();
    assert_eq!(answer["result"]["isError"], true);
    let text = answer["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("game.Workspace.Nothing"), "{text}");
}
