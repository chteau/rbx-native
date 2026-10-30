use std::collections::VecDeque;
use std::time::Duration;

use serde_json::{json, Value};

use super::super::tests::{scratch, Editor, NoClient, MAIN};
use super::super::{answer_timeout, Broker, Client};
use super::{parse_ask, Ask, Kind};

/// A client whose model answers from a script, keeping every request.
struct Scripted {
    answers: VecDeque<Value>,
    asked: Vec<Value>,
}

impl Scripted {
    fn new(answers: Vec<Value>) -> Scripted {
        Scripted {
            answers: answers.into(),
            asked: Vec::new(),
        }
    }
}

impl Client for Scripted {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        assert_eq!(method, "sampling/createMessage");
        self.asked.push(params);
        self.answers
            .pop_front()
            .ok_or_else(|| "the script ran out".to_owned())
    }
}

fn initialize(broker: &mut Broker, sampling: Value) {
    let line = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": { "sampling": sampling } },
    });
    let reply = broker.handle(&line.to_string(), &mut NoClient).unwrap();
    assert!(reply.contains("rbx-native"), "{reply}");
}

fn subagent(broker: &mut Broker, client: &mut dyn Client, kind: &str) -> (bool, String) {
    let line = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": { "name": "subagent", "arguments": {
            "subagent_type": kind,
            "description": "read main",
            "task": "What does Main print?",
        } },
    });
    let reply = broker.handle(&line.to_string(), client).unwrap();
    let reply: Value = serde_json::from_str(&reply).unwrap();
    let result = &reply["result"];
    (
        result["isError"].as_bool().unwrap(),
        result["content"][0]["text"].as_str().unwrap().to_owned(),
    )
}

fn text(text: &str) -> Value {
    json!({ "role": "assistant", "content": { "type": "text", "text": text } })
}

#[test]
fn without_sampling_it_is_refused() {
    let mut broker = Broker::new(scratch("subagent-nosampling"));
    initialize(&mut broker, Value::Null);
    let (failed, text) = subagent(&mut broker, &mut NoClient, "explore");
    assert!(failed);
    assert!(
        text.starts_with("Subagent execution is not available"),
        "{text}"
    );
}

#[test]
fn an_unknown_type_is_named() {
    let mut broker = Broker::new(scratch("subagent-unknown"));
    initialize(&mut broker, json!({}));
    let (failed, text) = subagent(&mut broker, &mut NoClient, "builder");
    assert!(failed);
    assert_eq!(
        text,
        "Unknown subagent type: \"builder\". Available types: screen_capture, unit_test, explore"
    );
}

#[test]
fn the_text_protocol_reads_a_script_then_answers() {
    let dir = scratch("subagent-text");
    let _editor = Editor::start(&dir, "Arena");
    let mut broker = Broker::new(dir);
    initialize(&mut broker, json!({}));
    let call = format!(
        "```json\n{{\"tool\": \"script_read\", \"arguments\": {{\"target_file\": \"{MAIN}\"}}}}\n```"
    );
    let done = text("{\"final\": \"It prints Arena\"}");
    let mut client = Scripted::new(vec![text(&call), done]);
    assert_eq!(
        subagent(&mut broker, &mut client, "explore"),
        (false, "It prints Arena".to_owned())
    );
    assert!(client.asked[0].get("tools").is_none());
    assert!(client.asked[0]["systemPrompt"]
        .as_str()
        .unwrap()
        .contains("script_read"));
    let fed = client.asked[1]["messages"][2].to_string();
    assert!(fed.contains("print(\\\"Arena\\\")"), "{fed}");
}

#[test]
fn the_tool_use_protocol_answers_with_tool_results() {
    let dir = scratch("subagent-tools");
    let _editor = Editor::start(&dir, "Lobby");
    let mut broker = Broker::new(dir);
    initialize(&mut broker, json!({ "tools": {} }));
    let mut client = Scripted::new(vec![
        json!({ "role": "assistant", "content": [{
            "type": "tool_use", "id": "u1", "name": "script_read",
            "input": { "target_file": MAIN },
        }] }),
        text("It prints Lobby"),
    ]);
    assert_eq!(
        subagent(&mut broker, &mut client, "explore"),
        (false, "It prints Lobby".to_owned())
    );
    let names: Vec<&str> = client.asked[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"script_read"));
    assert!(!names.contains(&"multi_edit"));
    let result = &client.asked[1]["messages"][2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["toolUseId"], "u1");
    assert_eq!(result["isError"], false);
    assert!(result["content"].to_string().contains("Lobby"), "{result}");
}

#[test]
fn a_tool_outside_the_list_is_refused_back_to_the_model() {
    let mut broker = Broker::new(scratch("subagent-refused"));
    initialize(&mut broker, json!({}));
    let mut client = Scripted::new(vec![
        text("{\"tool\": \"multi_edit\", \"arguments\": {}}"),
        text("{\"final\": \"gave up\"}"),
    ]);
    assert_eq!(
        subagent(&mut broker, &mut client, "explore"),
        (false, "gave up".to_owned())
    );
    let fed = client.asked[1]["messages"][2].to_string();
    assert!(
        fed.contains("Error: Tool \\\"multi_edit\\\" is not available to this subagent."),
        "{fed}"
    );
}

#[test]
fn each_tool_waits_as_long_as_it_needs() {
    assert_eq!(answer_timeout("execute_luau"), Duration::from_secs(120));
    assert_eq!(answer_timeout("start_stop_play"), Duration::from_secs(240));
    assert_eq!(answer_timeout("screen_capture"), Duration::from_secs(60));
    assert_eq!(
        answer_timeout("character_navigation"),
        Duration::from_secs(660)
    );
    assert_eq!(answer_timeout("script_read"), Duration::from_secs(30));
}

#[test]
fn each_kind_reaches_only_its_tools() {
    assert!(!Kind::Explore.allows("start_stop_play"));
    assert!(!Kind::Explore.allows("multi_edit"));
    assert!(Kind::ScreenCapture.allows("screen_capture"));
    assert!(!Kind::ScreenCapture.allows("start_stop_play"));
    assert!(Kind::UnitTest.allows("multi_edit"));
    assert!(Kind::UnitTest.allows("start_stop_play"));
    assert!(!Kind::UnitTest.allows("subagent"));
    assert_eq!(
        parse_ask("sure: {\"final\": \"x\"}"),
        Some(Ask::Final("x".to_owned()))
    );
}
