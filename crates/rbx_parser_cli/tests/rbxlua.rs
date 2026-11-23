//! Integration tests for the `rbxlua` binary: invoked as a real subprocess so
//! exit codes and stdout/stderr are covered, not just the library glue.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rbx_dom::{Ref, WeakDom};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/tests")
        .join(name)
}

/// Writes `source` to a fresh temp file and runs the built `rbxlua` binary
/// against `place` with it, plus any extra CLI flags.
fn run_script(place: &Path, source: &str, extra_args: &[&str]) -> Output {
    let dir = tempfile::tempdir().expect("temp dir");
    let script_path = dir.path().join("script.luau");
    std::fs::write(&script_path, source).expect("write script");

    let output = Command::new(env!("CARGO_BIN_EXE_rbxlua"))
        .arg(place)
        .arg(&script_path)
        .args(extra_args)
        .output()
        .expect("spawn rbxlua");

    // Keep the temp dir alive until the process has run and produced any
    // `--out` file the caller still needs to read.
    std::mem::forget(dir);
    output
}

fn find_named(dom: &WeakDom, name: &str) -> Option<Ref> {
    let mut stack = dom.root_refs().to_vec();
    while let Some(current) = stack.pop() {
        let Some(instance) = dom.get(current) else {
            continue;
        };
        if instance.name() == name {
            return Some(current);
        }
        stack.extend_from_slice(instance.children());
    }
    None
}

fn workspace_ref(dom: &WeakDom) -> Ref {
    dom.root_refs()
        .iter()
        .copied()
        .find(|referent| dom.get(*referent).is_some_and(|i| i.class() == "Workspace"))
        .expect("the place has a Workspace")
}

#[test]
fn prints_a_property_read_from_the_place() {
    let output = run_script(
        &fixture("TestPlace.rbxl"),
        "print(workspace.Baseplate.Size.X)",
        &[],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("2048"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn writes_the_mutated_dom_to_out() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out_path = dir.path().join("mutated.rbxl");

    let output = run_script(
        &fixture("TestPlace.rbxl"),
        // The fixture's one existing Part (Baseplate) carries several properties a
        // freshly scripted Part would not; the binary serializer fills those gaps
        // with each type's neutral value rather than requiring every instance of a
        // class to agree on which properties are present.
        r#"Instance.new("Part", workspace).Name = "FromLua""#,
        &["--out", out_path.to_str().unwrap()],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(&out_path).expect("read --out file");
    let dom = rbx_binary::deserialize(&bytes).expect("reload the written place");

    let model = find_named(&dom, "FromLua").expect("the scripted instance was written out");
    assert_eq!(dom.get(model).unwrap().class(), "Part");
    assert!(dom
        .get(workspace_ref(&dom))
        .unwrap()
        .children()
        .contains(&model));
}

#[test]
fn writes_the_mutated_dom_to_out_as_xml() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out_path = dir.path().join("mutated.rbxlx");

    let output = run_script(
        &fixture("TestPlace.rbxl"),
        r#"Instance.new("Part", workspace).Name = "FromLuaXml""#,
        &["--out", out_path.to_str().unwrap()],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let text = std::fs::read_to_string(&out_path).expect("read --out file");
    let dom = rbx_xml::deserialize(&text).expect("reload the written place");

    let part = find_named(&dom, "FromLuaXml").expect("the scripted instance was written out");
    assert_eq!(dom.get(part).unwrap().class(), "Part");
    assert!(dom
        .get(workspace_ref(&dom))
        .unwrap()
        .children()
        .contains(&part));
}

#[test]
fn a_lua_runtime_error_exits_with_code_one() {
    let output = run_script(
        &fixture("TestPlace.rbxl"),
        "workspace.Baseplate.Anchored = 5",
        &[],
    );

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("boolean expected"),
        "unexpected stderr: {stderr}"
    );
}

// `WeakDom::take_changes` drains a Vec in mutation order, so the listing is
// the same on every run and follows the script's own order.
#[test]
fn print_changes_order_is_stable_and_follows_the_script() {
    let script = r#"
        local a = Instance.new("Part", workspace)
        a.Name = "First"
        local b = Instance.new("Part", workspace)
        b.Name = "Second"
        a.Anchored = true
        b:Destroy()
    "#;
    let run = || {
        let output = run_script(&fixture("TestPlace.rbxl"), script, &["--print-changes"]);
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let first = run();
    assert_eq!(first, run());
    assert_eq!(first, run());
    assert_eq!(first.matches("Added").count(), 2, "{first}");
    assert!(
        first.find("Added").unwrap() < first.find("Removed").unwrap(),
        "{first}"
    );
    let last_added = first.rfind("Added").unwrap();
    assert!(first.find("Removed").unwrap() > last_added, "{first}");
}

#[test]
fn print_changes_lists_an_added_entry() {
    let output = run_script(
        &fixture("TestPlace.rbxl"),
        r#"Instance.new("Part", workspace)"#,
        &["--print-changes"],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Added"),
        "expected an Added entry in:\n{stdout}"
    );
}

/// A place holding one passing and one failing spec, written as XML.
fn spec_place(dir: &Path) -> PathBuf {
    let mut dom = WeakDom::new();
    let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
    for (name, body) in [
        (
            "good.spec",
            "it('adds', function() expect(1 + 1).to.equal(2) end)",
        ),
        (
            "bad.spec",
            "it('subtracts', function()\n expect(1 - 1).to.equal(1)\nend)",
        ),
    ] {
        let module = dom.new_instance("ModuleScript", name, Some(storage));
        dom.set_property(
            module,
            "Source",
            rbx_dom::Variant::String(format!("return function() {body} end")),
        )
        .unwrap();
    }
    let path = dir.join("specs.rbxlx");
    std::fs::write(&path, rbx_xml::serialize(&dom).unwrap()).unwrap();
    path
}

fn rbxlua(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rbxlua"))
        .args(args)
        .output()
        .expect("run rbxlua")
}

#[test]
fn test_reports_failures_with_exit_code_one_and_junit() {
    let dir = tempfile::tempdir().unwrap();
    let place = spec_place(dir.path());
    let junit = dir.path().join("out.xml");
    let output = rbxlua(&[
        "test",
        place.to_str().unwrap(),
        "--junit",
        junit.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1 passed, 1 failed, 0 skipped"), "{stdout}");
    assert!(
        stdout.contains("at ReplicatedStorage.bad.spec:2"),
        "{stdout}"
    );
    let xml = std::fs::read_to_string(junit).unwrap();
    assert!(xml.contains("<failure message=\"Expected 1 (number), got 0 (number)\">"));
}

#[test]
fn test_filter_runs_only_matching_tests_and_json_parses() {
    let dir = tempfile::tempdir().unwrap();
    let place = spec_place(dir.path());
    let output = rbxlua(&[
        "test",
        place.to_str().unwrap(),
        "--filter",
        "adds",
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        (value["passed"].as_u64(), value["failed"].as_u64()),
        (Some(1), Some(0))
    );
}

#[test]
fn test_reads_a_project_folder_and_runs_loren_specs_too() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/specs")).unwrap();
    std::fs::write(
        root.join("default.project.json"),
        r#"{ "name": "game", "tree": { "$className": "DataModel",
            "ReplicatedStorage": { "Specs": { "$path": "src/specs" } } } }"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/specs/Testez.spec.luau"),
        "return function()\n\tit('adds', function() expect(1 + 1).to.equal(2) end)\nend\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/specs/Loren.spec.luau"),
        "return function(t)\n\tt.it('fails', function()\n\t\tt.expect(1).toBe(2)\n\tend)\n\tt.it('later', function() end, { skip = true })\nend\n",
    )
    .unwrap();
    let output = rbxlua(&["test", root.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("1 passed, 1 failed, 1 skipped (2 files)"),
        "{stdout}"
    );
}

/// A baseplate and a spawn, written as a binary place.
fn loadtest_place(dir: &Path) -> PathBuf {
    use rbx_dom::{CFrameData, Variant, Vector3Data};
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    for class in ["Players", "ReplicatedStorage", "StarterPlayer"] {
        dom.new_instance(class, class, None);
    }
    for (class, y, size, height) in [
        ("Part", -10.0, 512.0, 20.0),
        ("SpawnLocation", 0.5, 12.0, 1.0),
    ] {
        let part = dom.new_instance(class, class, Some(workspace));
        let _ = dom.set_property(part, "Anchored", Variant::Bool(true));
        let _ = dom.set_property(
            part,
            "Size",
            Variant::Vector3(Vector3Data {
                x: size,
                y: height,
                z: size,
            }),
        );
        let _ = dom.set_property(
            part,
            "CFrame",
            Variant::CFrame(CFrameData {
                position: Vector3Data { x: 0.0, y, z: 0.0 },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        );
    }
    let path = dir.join("obby.rbxl");
    std::fs::write(&path, rbx_binary::serialize(&dom).unwrap()).unwrap();
    path
}

#[test]
fn loadtest_passes_and_reports_json() {
    let dir = tempfile::tempdir().unwrap();
    let place = loadtest_place(dir.path());
    let base = dir.path().join("report");
    let output = rbxlua(&[
        "loadtest",
        place.to_str().unwrap(),
        "--bots",
        "2",
        "--duration",
        "3s",
        "--json",
        "--out",
        base.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["passed"], true);
    assert_eq!(report["samples"].as_array().unwrap().len(), 3);
    assert_eq!(report["most_bots"], 2);
    let md = std::fs::read_to_string(base.with_extension("md")).unwrap();
    assert!(md.contains("kept 60 Hz with up to 2 bots"), "{md}");
    assert!(base.with_extension("json").exists());
}

#[test]
fn loadtest_exits_1_when_the_server_breaks() {
    let dir = tempfile::tempdir().unwrap();
    let place = loadtest_place(dir.path());
    // No server keeps 61 Hz.
    let output = rbxlua(&[
        "loadtest",
        place.to_str().unwrap(),
        "--bots",
        "1",
        "--duration",
        "3s",
        "--min-hz",
        "61",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("stopped keeping 60 Hz at 1 bots"),
        "{stdout}"
    );
    let usage = rbxlua(&["loadtest", place.to_str().unwrap(), "--bots", "many"]);
    assert_eq!(usage.status.code(), Some(2));
    let too_many = rbxlua(&["loadtest", place.to_str().unwrap(), "--bots", "51"]);
    assert_eq!(too_many.status.code(), Some(1));
}

/// The example game and its integration specs, copied into `dir`.
fn integration_project(dir: &Path) -> PathBuf {
    let from = fixture("local_run_integration");
    let root = dir.join("game");
    for sub in ["src/server", "src/specs"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
        for entry in std::fs::read_dir(from.join(sub)).unwrap() {
            let path = entry.unwrap().path();
            std::fs::copy(&path, root.join(sub).join(path.file_name().unwrap())).unwrap();
        }
    }
    std::fs::copy(
        from.join("default.project.json"),
        root.join("default.project.json"),
    )
    .unwrap();
    root
}

#[test]
fn test_runs_the_example_integration_specs_on_local_run() {
    let dir = tempfile::tempdir().unwrap();
    let junit = dir.path().join("out.xml");
    let output = rbxlua(&[
        "test",
        fixture("local_run_integration").to_str().unwrap(),
        "--junit",
        junit.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains("10 passed, 0 failed, 0 skipped (3 files)"),
        "{stdout}"
    );
    let xml = std::fs::read_to_string(junit).unwrap();
    assert!(
        xml.contains("<testsuite name=\"ServerStorage.Specs.Coins.integration.spec\" tests=\"4\""),
        "{xml}"
    );
    assert!(
        xml.contains("name=\"coin pickup › gives the coin to the player who walks into it\""),
        "{xml}"
    );
    assert!(!xml.contains("<failure"), "{xml}");
}

#[test]
fn a_failing_integration_spec_exits_one_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let root = integration_project(dir.path());
    std::fs::write(
        root.join("src/specs/Broken.integration.spec.luau"),
        "return function()\n\tit('expects a coin on join', function()\n\t\tlocal player = LocalRun.addPlayer()\n\t\texpect(player.leaderstats.Coins.Value).to.equal(1)\n\tend)\nend\n",
    )
    .unwrap();
    let junit = dir.path().join("out.xml");
    let output = rbxlua(&[
        "test",
        root.to_str().unwrap(),
        "--filter",
        "expects a coin",
        "--format",
        "json",
        "--junit",
        junit.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        (value["passed"].as_u64(), value["failed"].as_u64()),
        (Some(0), Some(1)),
        "{value}"
    );
    let xml = std::fs::read_to_string(junit).unwrap();
    assert!(
        xml.contains("<failure message=\"Expected 1 (number), got 0 (number)\">"),
        "{xml}"
    );
    assert!(
        xml.contains("ServerStorage.Specs.Broken.integration.spec:4"),
        "{xml}"
    );
}

#[test]
fn test_rejects_an_unknown_network_profile() {
    let output = rbxlua(&["test", "place.rbxl", "--network", "dialup"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "unknown network profile 'dialup' (one of none, fiber, broadband, wifi, 4g, 3g)"
        ),
        "{stderr}"
    );
}
