//! Integration tests for the `rbxaudit` binary: invoked as a real subprocess
//! so exit codes and stdout/stderr are covered.

use std::path::Path;
use std::process::{Command, Output};

use rbx_dom::{Variant, WeakDom};

const UNVALIDATED: &str =
    "game.ReplicatedStorage.R.OnServerEvent:Connect(function(p, n) p.leaderstats.Coins.Value = n end)";

/// A place with one server script; `remote` adds ReplicatedStorage.R.
fn place_with_script(source: &str, remote: bool) -> WeakDom {
    let mut dom = WeakDom::new();
    let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
    if remote {
        dom.new_instance("RemoteEvent", "R", Some(storage));
    }
    let service = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let script = dom.new_instance("Script", "Script", Some(service));
    dom.set_property(script, "Source", Variant::String(source.to_string()))
        .expect("set Source");
    dom
}

fn audit(dom: &WeakDom) -> Output {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("place.rbxlx");
    std::fs::write(&path, rbx_xml::serialize(dom).expect("serialize")).expect("write place");
    run(&path)
}

fn run(path: &Path) -> Output {
    run_with(&[], path)
}

fn run_with(flags: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rbxaudit"))
        .args(flags)
        .arg(path)
        .output()
        .expect("spawn rbxaudit")
}

/// The generated pre-publish fixture place (`rbx_audit`'s `tests/prepublish.rs`).
fn prepublish_fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/prepublish/PrePublish.rbxlx")
}

#[test]
fn a_high_finding_exits_one() {
    let output = audit(&place_with_script(UNVALIDATED, true));

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout
            .lines()
            .any(|l| l.starts_with("high") && l.contains("unvalidated-argument")),
        "stdout: {stdout}"
    );
}

#[test]
fn a_clean_place_exits_zero() {
    let output = audit(&place_with_script("print(1)", false));

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("no findings"));
}

#[test]
fn a_missing_file_exits_two() {
    let dir = tempfile::tempdir().expect("temp dir");
    let output = run(&dir.path().join("missing.rbxlx"));

    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.is_empty());
}

#[test]
fn prepublish_offline_reports_every_rule_and_exits_one() {
    let output = run_with(&["--prepublish", "--offline"], &prepublish_fixture());

    // A malformed asset id and an over-limit-free place: High from the id.
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "high   broken-asset Workspace.Props.BadMesh",
        "medium broken-asset Workspace.Props.EmptyMesh",
        "low    assets-not-checked Workspace 4 asset id(s) not checked",
        "low    deprecated-api ServerScriptService.Deprecated:5",
        "medium datastore-without-pcall ServerScriptService.Data:10",
        "medium unanchored-parts Workspace.Crates 2 unanchored",
        "medium heavy-geometry Workspace.Props.BigUnion",
        "low    streaming-index StarterPlayer.StarterPlayerScripts.Streaming:2",
        "low    no-server-listener ReplicatedStorage.Orphan",
    ] {
        assert!(
            stdout.lines().any(|l| l.starts_with(expected)),
            "{expected}\n{stdout}"
        );
    }
}

#[test]
fn prepublish_json_is_one_object_for_ci() {
    let output = run_with(
        &["--prepublish", "--offline", "--json"],
        &prepublish_fixture(),
    );

    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let findings = report["findings"].as_array().unwrap();
    assert!(findings.iter().any(|f| f["check"] == "broken-asset"
        && f["severity"] == "high"
        && f["path"] == "Workspace.Props.BadMesh"
        && f["line"].is_null()));
    assert!(findings
        .iter()
        .any(|f| f["check"] == "deprecated-api" && f["line"] == 5));
    assert_eq!(report["summary"]["high"], 1);
}

#[test]
fn offline_without_prepublish_is_bad_usage() {
    let output = run_with(&["--offline"], &prepublish_fixture());

    assert_eq!(output.status.code(), Some(2));
}
