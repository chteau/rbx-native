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
