//! Every Luau file and script the repo ships, read in and written back: the
//! text must come out as it went in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::super::codegen::{self, fingerprint};
use super::super::Graph;
use super::{emit, import, same};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !path.ends_with("target") && !path.ends_with(".git") && !path.ends_with(".claude") {
                walk(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

/// (name, source) of every script the corpus holds.
fn sources() -> Vec<(String, String)> {
    let mut files = Vec::new();
    walk(&root().join("assets"), &mut files);
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let name = path
            .strip_prefix(root())
            .unwrap_or(&path)
            .display()
            .to_string();
        match path.extension().and_then(|e| e.to_str()) {
            Some("lua" | "luau") => {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((name, text));
                }
            }
            Some("rbxl" | "rbxm") if name.contains("assets/tests/") => {
                scripts_in(&name, &path, &mut out);
            }
            _ => {}
        }
    }
    let edge = include_str!("corpus_edge.luau");
    out.push(("corpus_edge.luau".to_owned(), edge.to_owned()));
    out.push(("corpus_edge (CRLF)".to_owned(), edge.replace('\n', "\r\n")));
    out.push((
        "corpus_edge (spaces)".to_owned(),
        edge.replace('\t', "    "),
    ));
    out
}

fn scripts_in(name: &str, path: &Path, out: &mut Vec<(String, String)>) {
    let Ok(dom) = rbx_viewer::read_place(path) else {
        return;
    };
    let mut todo: Vec<rbx_dom::Ref> = dom.root_refs().to_vec();
    while let Some(r) = todo.pop() {
        let Some(inst) = dom.get(r) else {
            continue;
        };
        todo.extend_from_slice(inst.children());
        if !matches!(inst.class(), "Script" | "LocalScript" | "ModuleScript") {
            continue;
        }
        if let Some(src) = crate::script_editor::source::read(&dom, r) {
            out.push((format!("{name}:{}", inst.name()), src));
        }
    }
}

/// Kinds, wire count and the fingerprint of each top-level node, with
/// positions and ids left out.
fn shape(graph: &Graph) -> (BTreeMap<String, usize>, usize, Vec<u64>) {
    let mut kinds = BTreeMap::new();
    for n in &graph.nodes {
        *kinds.entry(n.kind.clone()).or_default() += 1;
    }
    let mut prints: Vec<u64> = graph
        .nodes
        .iter()
        .filter(|n| {
            matches!(n.kind.as_str(), "start")
                || graph
                    .kind_of(n.id)
                    .is_some_and(|k| matches!(k.code, super::super::catalog::Code::Event(_)))
        })
        .map(|n| fingerprint(graph, n.id))
        .collect();
    prints.sort_unstable();
    (kinds, graph.wires.len(), prints)
}

fn first_diff(a: &str, b: &str) -> String {
    for (n, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {}: {x:?} vs {y:?}", n + 1);
        }
    }
    format!("length {} vs {}", a.lines().count(), b.lines().count())
}

fn check(src: &str) -> Result<(), String> {
    emit::take_fallbacks();
    let got = import(src);
    let fallbacks = emit::take_fallbacks();
    let out = codegen::compile_with(&got.graph, &got.origins)
        .map_err(|p| format!("compile with origins failed: {} problems", p.len()))?;
    if out != src {
        return Err(format!("round trip: {}", first_diff(src, &out)));
    }
    let plain = codegen::compile(&got.graph)
        .map_err(|p| format!("plain compile failed: {} problems", p.len()))?;
    if !same(&plain, src) {
        return Err(format!("not same: {}", first_diff(src, &plain)));
    }
    let again = import(&plain);
    if shape(&again.graph) != shape(&got.graph) {
        return Err("re-import has a different shape".to_owned());
    }
    if let Some(first) = fallbacks.first() {
        return Err(format!("{} kept as Luau Code: {first}", fallbacks.len()));
    }
    Ok(())
}

#[test]
fn corpus_round_trips() {
    let all = sources();
    let (mut pass, mut total, mut broken) = (0, 0, Vec::new());
    let mut failures = Vec::new();
    for (name, src) in &all {
        if full_moon::parse(src).is_err() {
            broken.push(name.clone());
            continue;
        }
        total += 1;
        match check(src) {
            Ok(()) => pass += 1,
            Err(why) => failures.push(format!("{name}: {why}")),
        }
    }
    println!("corpus: {pass}/{total} pass");
    println!("unparseable: {broken:?}");
    for f in &failures {
        println!("FAIL {}", f.chars().take(1500).collect::<String>());
    }
    assert!(failures.is_empty(), "{} corpus files fail", failures.len());
}
