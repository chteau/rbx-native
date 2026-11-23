use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Value};

use super::super::call;
use super::super::tests::{place, run, studio};
use crate::mcp::{Insertion, Session};
use crate::properties::Properties;

mod images;
mod import;
mod insert;
mod search;

thread_local! {
    /// The mock `apis.roblox.com` this thread's calls go to.
    pub(super) static MOCK_ROOT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// One request the mock saw: `METHOD /path?query` and its body.
pub(super) type Seen = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// Serves `route` on a loopback port for this thread's asset tools; `@` in
/// an answer stands for the server's own root, for delivery's CDN links.
pub(super) fn mock(route: impl Fn(&str, &[u8]) -> (u16, Vec<u8>) + Send + 'static) -> Seen {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let root = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = seen.clone();
    let base = root.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let target = line
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ");
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some((name, value)) = header.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let (status, answer) = route(&target, &body);
            log.lock().unwrap().push((target, body));
            let answer = match String::from_utf8(answer) {
                Ok(text) => text.replace('@', &base).into_bytes(),
                Err(bytes) => bytes.into_bytes(),
            };
            let mut stream = reader.into_inner();
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                answer.len()
            );
            let _ = stream.write_all(&answer);
        }
    });
    MOCK_ROOT.with(|mock| *mock.borrow_mut() = Some(root));
    seen
}

pub(super) fn json_body(value: Value) -> (u16, Vec<u8>) {
    (200, value.to_string().into_bytes())
}

pub(super) const INTROSPECT: &str =
    r#"{"name":"k","authorizedUserId":5,"scopes":[],"enabled":true,"expired":false}"#;

/// `version 1.01` text mesh spanning 4 x 2 x 6 studs.
pub(super) const MESH: &[u8] =
    b"version 1.01\n1\n[0,0,0][0,1,0][0,0,0][4,0,0][0,1,0][0,0,0][0,2,6][0,1,0][0,0,0]";

pub(super) const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

pub(super) fn model_bytes() -> Vec<u8> {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Crate", None);
    dom.new_instance("Part", "Lid", Some(model));
    rbx_binary::serialize(&dom).unwrap()
}

/// Calls `insert_asset` and runs its download, as the shell would.
pub(super) fn insert(dom: &WeakDom, arguments: Value) -> Result<Insertion, String> {
    let properties = Properties::new(ReflectionDatabase::embedded());
    let studio = studio();
    let mut session = Session::new(dom, &properties, &studio);
    let result = call(
        &json!({ "name": "insert_asset", "arguments": arguments }),
        &mut session,
    )
    .unwrap();
    match session.inserting.take() {
        Some(fetch) => fetch(),
        None => Err(result["content"][0]["text"].as_str().unwrap().to_owned()),
    }
}

/// Delivery's answer for asset 1818 of type `type_id`, holding `bytes`.
pub(super) fn deliver(type_id: u32, bytes: Vec<u8>) -> Seen {
    mock(move |target, _| match target {
        t if t.contains("/asset-delivery/") => (401, Vec::new()),
        t if t.contains("/asset-delivery-api/v1/assetId/1818") => json_body(json!({
            "location": "@/cdn/1818",
            "assetTypeId": type_id,
        })),
        t if t.contains("/cdn/1818") => (200, bytes.clone()),
        t if t.contains("/toolbox-service/v2/assets/1818") => {
            json_body(json!({ "asset": { "id": 1818, "name": "Rusty Crate" } }))
        }
        _ => (404, Vec::new()),
    })
}

#[test]
fn every_asset_tool_is_listed_with_a_studio_id() {
    let tools = super::super::list();
    for name in [
        "search_asset",
        "insert_asset",
        "upload_image",
        "store_image",
        "generate_mesh",
        "generate_material",
        "generate_procedural_model",
        "segment_mesh",
        "generate_texture",
        "wait_job_finished",
    ] {
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("{name} is not listed"));
        assert!(tool["inputSchema"]["properties"]["studio_id"].is_object());
    }
}

#[test]
fn generation_says_why_it_cannot_run() {
    let dom = place();
    for (tool, arguments) in [
        ("generate_mesh", json!({ "textPrompt": "a crate" })),
        ("generate_procedural_model", json!({ "prompt": "a crate" })),
        (
            "generate_material",
            json!({ "materialPattern": "Regular", "materialId": "m", "baseMaterial": "Brick", "materialDescription": "red" }),
        ),
    ] {
        let (error, text) = run(&dom, tool, arguments);
        assert!(error, "{tool}");
        assert!(text.contains("not available in rbx-native"), "{text}");
    }
    let (error, text) = run(&dom, "wait_job_finished", json!({ "jobId": "j1" }));
    assert!(error);
    assert!(text.starts_with("No job found with id: j1."), "{text}");
}

#[test]
fn mesh_edit_tools_carry_studios_titles_and_annotations() {
    let tools = super::super::list();
    for (name, title) in [
        ("segment_mesh", "Mesh Segmentation"),
        ("generate_texture", "Texture Generation"),
    ] {
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap();
        assert_eq!(tool["title"], title);
        assert_eq!(tool["annotations"]["title"], title);
        assert_eq!(tool["annotations"]["destructiveHint"], true);
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
        assert!(
            tool["inputSchema"]["properties"]["studio_id"]["description"]
                .as_str()
                .unwrap()
                .starts_with("Which editor to act on")
        );
    }
}

#[test]
fn mesh_edit_tools_validate_before_they_refuse() {
    let mut dom = place();
    let workspace = dom.root_refs()[0];
    dom.new_instance("MeshPart", "Rock", Some(workspace));
    let segment = |arguments| run(&dom, "segment_mesh", arguments);
    let texture = |arguments| run(&dom, "generate_texture", arguments);

    let (error, text) = segment(json!({ "instance_path": "Workspace.Rock" }));
    assert!(
        error && text.contains("missing the `parts` argument"),
        "{text}"
    );
    let (_, text) = segment(
        json!({ "instance_path": "Workspace.Rock", "parts": ["a", "b", "c", "d", "e", "f"] }),
    );
    assert!(text.contains("at most 5"), "{text}");
    let (_, text) = segment(json!({ "parts": ["a"] }));
    assert!(
        text.contains("not exactly one MeshPart or Model (0 selected)"),
        "{text}"
    );
    let (_, text) = segment(json!({ "instance_path": "Workspace.Nope", "parts": ["a"] }));
    assert!(
        text.contains("No instance found at path: Workspace.Nope"),
        "{text}"
    );
    let (_, text) = segment(json!({ "instance_path": "Workspace.Tower.Door", "parts": ["a"] }));
    assert!(
        text.contains("is a Part, not a MeshPart or Model"),
        "{text}"
    );
    for path in ["Workspace.Rock", "Workspace.Tower"] {
        let (error, text) = segment(json!({ "instance_path": path, "parts": ["a", "b"] }));
        assert!(
            error && text.contains("segment_mesh is not available in rbx-native"),
            "{text}"
        );
    }

    let (_, text) = texture(json!({ "instance_path": "Workspace.Rock" }));
    assert!(text.contains("missing the `textPrompt` argument"), "{text}");
    let (_, text) =
        texture(json!({ "textPrompt": "rust", "instance_path": "Workspace.Tower.Door" }));
    assert!(text.contains("not a MeshPart or Model"), "{text}");
    let (error, text) = texture(json!({ "textPrompt": "rust", "instance_path": "Workspace.Rock" }));
    assert!(
        error && text.contains("generate_texture is not available in rbx-native"),
        "{text}"
    );
    let (_, text) =
        texture(json!({ "hintImage": {}, "textPrompt": "", "instance_path": "Workspace.Rock" }));
    assert!(text.contains("generate_texture is not available"), "{text}");
}

/// `segment_mesh` without `instance_path`, with `selected` (dotted paths
/// under Workspace) as the Explorer selection.
fn segment_selected(dom: &WeakDom, selected: &[&str]) -> String {
    let properties = Properties::new(ReflectionDatabase::embedded());
    let studio = studio();
    let mut session = Session::new(dom, &properties, &studio);
    session.selection = selected
        .iter()
        .map(|path| super::super::resolve(dom, path).unwrap())
        .collect();
    let result = call(
        &json!({ "name": "segment_mesh", "arguments": { "parts": ["a"] } }),
        &mut session,
    )
    .unwrap();
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

#[test]
fn mesh_edit_tools_fall_back_to_exactly_one_selected_mesh_part_or_model() {
    let mut dom = place();
    let workspace = dom.root_refs()[0];
    dom.new_instance("MeshPart", "Rock", Some(workspace));
    for one in ["Workspace.Rock", "Workspace.Tower"] {
        let text = segment_selected(&dom, &[one]);
        assert!(text.contains("segment_mesh is not available"), "{text}");
    }
    let text = segment_selected(&dom, &["Workspace.Tower.Door"]);
    assert!(
        text.contains("not exactly one MeshPart or Model (1 selected)"),
        "{text}"
    );
    let text = segment_selected(&dom, &["Workspace.Rock", "Workspace.Tower"]);
    assert!(text.contains("(2 selected)"), "{text}");
}
