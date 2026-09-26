use std::path::PathBuf;

use rbx_dom::{Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;
use serde_json::json;

use super::{diagnostics, path, start, uri, wait, Mirror};

#[test]
fn a_path_survives_the_round_trip_through_a_uri() {
    let folder = std::env::temp_dir().join("with space").join("12.luau");
    let uri = uri(&folder);
    assert!(uri.starts_with("file://"), "{uri}");
    assert!(uri.contains("with%20space"), "{uri}");
    assert_eq!(path(&uri), Some(folder));
    assert_eq!(path("https://example.com/12.luau"), None);
}

/// Needs a real server: `RBX_STUDIO_LUAU_LSP=/path/to/luau-lsp cargo test
/// -p rbx_studio -- --ignored luau_lsp`, and the network on a first run for
/// the definitions file.
#[test]
#[ignore]
fn a_real_server_resolves_the_place_and_reports_problems_in_scripts_nobody_opened() {
    let mut dom = WeakDom::new();
    let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
    let module = dom.new_instance("ModuleScript", "Greeter", Some(storage));
    let source = "return { greet = function(): string return 'hi' end }";
    let _ = dom.set_property(module, "Source", Variant::String(source.into()));
    let main = dom.new_instance("Script", "Main", Some(storage));
    let source = "local Greeter = require(script.Parent.Greeter)\nlocal n: number = 1 + nil\nprint(Greeter.greet(), n)\n";
    let _ = dom.set_property(main, "Source", Variant::String(source.into()));

    let root: PathBuf = std::env::temp_dir().join(format!("rbx-luau-real-{}", std::process::id()));
    let mut mirror = Mirror::new(root.clone());
    mirror.sync(&dom, &ReflectionDatabase::embedded()).unwrap();
    let client = start(&root).expect("server starts");

    let reply =
        wait(client.request("workspace/diagnostic", json!({"previousResultIds": []}))).unwrap();
    let scripts = diagnostics::parse(&reply, &mirror);
    assert_eq!(scripts.len(), 1, "{scripts:?}");
    assert_eq!(scripts[0].0, main);
    assert!(
        scripts[0].1.iter().any(|p| p.range.start.line == 1),
        "the `1 + nil` line: {scripts:?}"
    );

    // `Greeter.` completes from the module through the sourcemap.
    let main_uri = uri(&mirror.path_of(main));
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": main_uri, "languageId": "luau", "version": 1,
            "text": "local Greeter = require(script.Parent.Greeter)\nGreeter.\n"}}),
    );
    let reply = wait(client.request(
        "textDocument/completion",
        json!({"textDocument": {"uri": main_uri}, "position": {"line": 1, "character": 8}}),
    ))
    .unwrap();
    let labels: Vec<&str> = reply
        .as_array()
        .or(reply["items"].as_array())
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"greet"), "{labels:?}");
}
