//! End-to-end tests for the `rbxmerge` driver: direct calls against its exit
//! codes, sidecar and output format, and a real `git merge` through the
//! driver that `rbxmerge install` registers.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use base64::Engine as _;
use rbx_dom::{Instance, Ref, UniqueId, Variant, WeakDom};

fn uid(n: u32) -> Variant {
    Variant::UniqueId(UniqueId {
        index: n,
        time: 1,
        random: 7,
    })
}

fn add(dom: &mut WeakDom, r: u32, class: &str, name: &str, parent: Option<Ref>, id: u32) -> Ref {
    let r = Ref::new(r);
    let mut instance = Instance::new(r, class, name);
    instance.properties_mut().insert("UniqueId".into(), uid(id));
    dom.insert(instance);
    if let Some(p) = parent {
        dom.set_parent(r, Some(p));
    }
    r
}

/// Workspace > A and B, each with a Transparency. `offset` shifts every
/// referent so the three sides never share one.
fn place(offset: u32, a: f32, b: f32) -> WeakDom {
    let mut dom = WeakDom::new();
    let ws = add(&mut dom, offset + 1, "Workspace", "Workspace", None, 1);
    let part_a = add(&mut dom, offset + 2, "Part", "A", Some(ws), 2);
    let part_b = add(&mut dom, offset + 3, "Part", "B", Some(ws), 3);
    dom.get_mut(part_a)
        .unwrap()
        .properties_mut()
        .insert("Transparency".into(), Variant::Float32(a));
    dom.get_mut(part_b)
        .unwrap()
        .properties_mut()
        .insert("Transparency".into(), Variant::Float32(b));
    dom
}

fn binary(dom: &WeakDom) -> Vec<u8> {
    rbx_binary::serialize(dom).unwrap()
}

fn find(dom: &WeakDom, path: &str) -> Option<Ref> {
    let mut level: Vec<Ref> = dom.root_refs().to_vec();
    let mut found = None;
    for name in path.split('.') {
        let r = level
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.name() == name))?;
        level = dom.get(r)?.children().to_vec();
        found = Some(r);
    }
    found
}

fn transparency(dom: &WeakDom, path: &str) -> Option<f32> {
    let r = find(dom, path)?;
    match dom.get(r)?.properties().get("Transparency")? {
        Variant::Float32(v) => Some(*v),
        _ => None,
    }
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rbxmerge"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap()
}

fn git_ok(dir: &Path, args: &[&str]) {
    let out = git(dir, args);
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn non_overlapping_edits_merge_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(p.join("base.rbxl"), binary(&place(0, 0.0, 0.0))).unwrap();
    fs::write(p.join("ours.rbxl"), binary(&place(100, 0.5, 0.0))).unwrap();
    fs::write(p.join("theirs.rbxl"), binary(&place(200, 0.0, 0.7))).unwrap();

    let out = run(p, &["base.rbxl", "ours.rbxl", "theirs.rbxl", "place.rbxl"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");

    let merged = rbx_binary::deserialize(&fs::read(p.join("ours.rbxl")).unwrap()).unwrap();
    assert_eq!(transparency(&merged, "Workspace.A"), Some(0.5));
    assert_eq!(transparency(&merged, "Workspace.B"), Some(0.7));
    assert!(!p.join("place.rbxl.merge.json").exists());
}

#[test]
fn conflicting_edit_writes_sidecar_and_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let base = binary(&place(0, 0.0, 0.0));
    let theirs = binary(&place(200, 0.9, 0.0));
    fs::write(p.join("base.rbxl"), &base).unwrap();
    fs::write(p.join("ours.rbxl"), binary(&place(100, 0.5, 0.0))).unwrap();
    fs::write(p.join("theirs.rbxl"), &theirs).unwrap();

    let out = run(p, &["base.rbxl", "ours.rbxl", "theirs.rbxl", "place.rbxl"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");

    let merged = rbx_binary::deserialize(&fs::read(p.join("ours.rbxl")).unwrap()).unwrap();
    assert_eq!(transparency(&merged, "Workspace.A"), Some(0.5));

    let sidecar: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(p.join("place.rbxl.merge.json")).unwrap())
            .unwrap();
    assert_eq!(sidecar["version"], 1);
    assert_eq!(sidecar["file"], "place.rbxl");
    let engine = base64::engine::general_purpose::STANDARD;
    assert_eq!(
        engine.decode(sidecar["base"].as_str().unwrap()).unwrap(),
        base
    );
    assert_eq!(
        engine.decode(sidecar["theirs"].as_str().unwrap()).unwrap(),
        theirs
    );
    let conflict = &sidecar["conflicts"][0];
    assert_eq!(conflict["path"], "Workspace.A");
    assert_eq!(conflict["class"], "Part");
    assert_eq!(conflict["target"], "Transparency");
}

#[test]
fn garbage_input_exits_two_and_leaves_ours_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let ours = binary(&place(100, 0.5, 0.0));
    fs::write(p.join("base.rbxl"), b"definitely not a place").unwrap();
    fs::write(p.join("ours.rbxl"), &ours).unwrap();
    fs::write(p.join("theirs.rbxl"), binary(&place(200, 0.0, 0.7))).unwrap();

    let out = run(p, &["base.rbxl", "ours.rbxl", "theirs.rbxl", "place.rbxl"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(fs::read(p.join("ours.rbxl")).unwrap(), ours);
}

#[test]
fn xml_inputs_produce_xml_output() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(
        p.join("base.rbxlx"),
        rbx_xml::serialize(&place(0, 0.0, 0.0)).unwrap(),
    )
    .unwrap();
    fs::write(
        p.join("ours.rbxlx"),
        rbx_xml::serialize(&place(100, 0.5, 0.0)).unwrap(),
    )
    .unwrap();
    fs::write(
        p.join("theirs.rbxlx"),
        rbx_xml::serialize(&place(200, 0.0, 0.7)).unwrap(),
    )
    .unwrap();

    let out = run(
        p,
        &["base.rbxlx", "ours.rbxlx", "theirs.rbxlx", "place.rbxlx"],
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");

    let written = fs::read_to_string(p.join("ours.rbxlx")).unwrap();
    assert!(written.starts_with("<roblox"), "{written}");
    let merged = rbx_xml::deserialize(&written).unwrap();
    assert_eq!(transparency(&merged, "Workspace.A"), Some(0.5));
    assert_eq!(transparency(&merged, "Workspace.B"), Some(0.7));
}

#[test]
fn install_registers_driver_and_is_idempotent() {
    if !git_available() {
        eprintln!("git not available; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git_ok(p, &["init", "-q"]);

    let out = run(p, &["install"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");

    let attributes = fs::read_to_string(p.join(".gitattributes")).unwrap();
    for line in [
        "*.rbxl merge=rbx",
        "*.rbxm merge=rbx",
        "*.rbxlx merge=rbx",
        "*.rbxmx merge=rbx",
    ] {
        assert!(attributes.lines().any(|l| l.trim() == line), "{attributes}");
    }
    let driver = git(p, &["config", "--get", "merge.rbx.driver"]);
    assert!(
        String::from_utf8_lossy(&driver.stdout).contains("%O %A %B %P"),
        "{driver:?}"
    );

    let out = run(p, &["install"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        fs::read_to_string(p.join(".gitattributes")).unwrap(),
        attributes
    );
}

/// Repo on `main` with `.gitattributes` installed and `place.rbxl` committed
/// with the given Transparency pair.
fn repo_with_place(p: &Path, a: f32, b: f32) {
    git_ok(p, &["init", "-q"]);
    git_ok(p, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    assert!(run(p, &["install"]).status.success());
    fs::write(p.join("place.rbxl"), binary(&place(0, a, b))).unwrap();
    git_ok(p, &["add", "-A"]);
    git_ok(p, &["commit", "-q", "-m", "base"]);
}

fn commit_place(p: &Path, a: f32, b: f32, message: &str) {
    fs::write(p.join("place.rbxl"), binary(&place(0, a, b))).unwrap();
    git_ok(p, &["add", "-A"]);
    git_ok(p, &["commit", "-q", "-m", message]);
}

#[test]
fn git_merge_of_non_overlapping_edits_succeeds() {
    if !git_available() {
        eprintln!("git not available; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    repo_with_place(p, 0.0, 0.0);

    git_ok(p, &["checkout", "-q", "-b", "other"]);
    commit_place(p, 0.0, 0.7, "edit B");
    git_ok(p, &["checkout", "-q", "main"]);
    commit_place(p, 0.5, 0.0, "edit A");

    git_ok(p, &["merge", "-q", "--no-edit", "other"]);

    let merged = rbx_binary::deserialize(&fs::read(p.join("place.rbxl")).unwrap()).unwrap();
    assert_eq!(transparency(&merged, "Workspace.A"), Some(0.5));
    assert_eq!(transparency(&merged, "Workspace.B"), Some(0.7));
}

#[test]
fn git_merge_of_conflicting_edits_writes_sidecar_at_repo_top() {
    if !git_available() {
        eprintln!("git not available; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    repo_with_place(p, 0.0, 0.0);

    git_ok(p, &["checkout", "-q", "-b", "other"]);
    commit_place(p, 0.9, 0.0, "edit A one way");
    git_ok(p, &["checkout", "-q", "main"]);
    commit_place(p, 0.5, 0.0, "edit A the other way");

    let merge = git(p, &["merge", "-q", "--no-edit", "other"]);
    assert!(!merge.status.success(), "{merge:?}");
    assert!(p.join("place.rbxl.merge.json").exists());
}
