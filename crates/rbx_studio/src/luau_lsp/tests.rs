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

/// Runs the bundled server (or `RBX_STUDIO_LUAU_LSP`'s). Needs the network
/// only when the cache has no copy of Roblox's API yet, the same network
/// `build.rs` needed to bundle the server in the first place.
#[test]
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

/// TestEZ's globals come from `testez.d.luau`: a spec using them is clean,
/// while a misspelt one still is not.
#[test]
fn a_spec_file_knows_testez_globals() {
    let mut dom = WeakDom::new();
    let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
    let spec = dom.new_instance("ModuleScript", "Math.spec", Some(storage));
    let source = "--!strict\nreturn function()\n\tdescribe(\"math\", function()\n\t\tbeforeEach(function() end)\n\t\tit(\"adds\", function()\n\t\t\texpect(1 + 1).to.equal(2)\n\t\t\texpect(2).never.to.be.a(\"string\")\n\t\t\texpect(3).toBeGreaterThan(2)\n\t\tend)\n\t\tit.skip(\"later\", function() end)\n\tend)\nend\n";
    let _ = dom.set_property(spec, "Source", Variant::String(source.into()));
    let typo = dom.new_instance("ModuleScript", "Typo.spec", Some(storage));
    let source = "return function()\n\tdescrbe(\"x\", function() end)\nend\n";
    let _ = dom.set_property(typo, "Source", Variant::String(source.into()));

    let root: PathBuf =
        std::env::temp_dir().join(format!("rbx-luau-testez-{}", std::process::id()));
    let mut mirror = Mirror::new(root.clone());
    mirror.sync(&dom, &ReflectionDatabase::embedded()).unwrap();
    let client = start(&root).expect("server starts");
    // The server analyses the spec files in the background: ask again until
    // the typo is flagged, rather than trusting one early, partial answer.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    let scripts = loop {
        let reply = wait(client.request("workspace/diagnostic", json!({"previousResultIds": []})));
        let scripts = reply
            .map(|reply| diagnostics::parse(&reply, &mirror))
            .unwrap_or_default();
        if scripts.iter().any(|(script, _)| *script == typo) || std::time::Instant::now() > deadline
        {
            break scripts;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    assert!(
        scripts.iter().all(|(script, _)| *script != spec),
        "the spec should be clean: {scripts:?}"
    );
    assert!(
        scripts.iter().any(|(script, _)| *script == typo),
        "the typo should be flagged: {scripts:?}"
    );
}
