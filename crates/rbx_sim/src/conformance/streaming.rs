//! The streaming probe: a place with `Workspace.StreamingEnabled` and
//! probe instances (named `S_...`) at known distances from the spawn, whose
//! scripts (`conformance/streaming/`) move the player's focus about and
//! print what the client had at each point. The `record_streaming` example
//! records it on a real server, one place per [`VARIANTS`] entry, into
//! `conformance/goldens/streaming/<variant>.json`; [`run_in_sim`] runs the
//! same place in a [`Session`].

use std::collections::BTreeMap;

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use serde_json::Value;

use super::{bare_place, child, service};
use crate::{Peer, Session};

pub const PREFIX: &str = "STREAMING ";
pub const DONE: &str = "STREAMING_DONE";

const SERVER: &str = include_str!("../../conformance/streaming/Server.luau");
const CLIENT: &str = include_str!("../../conformance/streaming/Client.luau");
const FIRST: &str = include_str!("../../conformance/streaming/First.luau");

/// Each probe place's `Workspace` settings on top of `StreamingEnabled`.
pub const VARIANTS: &[(&str, &[(&str, i32)])] = &[
    ("Default", &[]),
    (
        "Opportunistic",
        &[("StreamOutBehavior", 2), ("StreamingTargetRadius", 256)],
    ),
    ("MinimumRadiusPause", &[("StreamingIntegrityMode", 2)]),
    ("PauseOutsideLoadedArea", &[("StreamingIntegrityMode", 3)]),
];

/// Distances along +X of the `P` parts.
const RING: &[f32] = &[
    8.0, 16.0, 32.0, 48.0, 56.0, 64.0, 72.0, 80.0, 96.0, 112.0, 128.0, 160.0, 192.0, 224.0, 256.0,
    288.0, 320.0, 384.0, 448.0, 512.0, 640.0, 768.0, 896.0, 960.0, 1024.0, 1088.0, 1152.0, 1280.0,
    1536.0, 2048.0, 3000.0,
];
/// Distances of the parts along -X (`N`), +Z (`Z`), the diagonal (`D`)
/// and up (`H`).
const AXES: &[f32] = &[64.0, 128.0, 256.0, 512.0, 1024.0, 1536.0];
/// Offsets along +Z of the clusters around a far point.
const CLUSTER: &[f32] = &[0.0, 32.0, 64.0, 128.0, 256.0, 512.0];

fn at(x: f32, y: f32, z: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    })
}

fn part(dom: &mut WeakDom, name: &str, parent: Ref, [x, y, z]: [f32; 3]) -> Ref {
    let r = child(dom, "Part", &format!("S_{name}"), parent);
    let _ = dom.set_property(r, "Anchored", Variant::Bool(true));
    let _ = dom.set_property(
        r,
        "Size",
        Variant::Vector3(Vector3Data {
            x: 2.0,
            y: 2.0,
            z: 2.0,
        }),
    );
    let _ = dom.set_property(r, "CFrame", at(x, y, z));
    r
}

fn model(dom: &mut WeakDom, name: &str, mode: u32, parts: &[[f32; 3]], workspace: Ref) -> Ref {
    let m = child(dom, "Model", &format!("S_{name}"), workspace);
    let _ = dom.set_property(m, "ModelStreamingMode", Variant::Enum(mode));
    for (i, p) in parts.iter().enumerate() {
        part(dom, &format!("{name}_{i}"), m, *p);
    }
    m
}

/// The probe place for variant `name` (a [`VARIANTS`] entry).
pub fn place(name: &str) -> Option<WeakDom> {
    let (_, settings) = VARIANTS.iter().find(|(n, _)| *n == name)?;
    let mut dom = bare_place();
    let workspace = service(&mut dom, "Workspace");
    let _ = dom.set_property(workspace, "StreamingEnabled", Variant::Bool(true));
    for (key, value) in *settings {
        let value = if key.ends_with("Radius") {
            Variant::Int32(*value)
        } else {
            Variant::Enum(*value as u32)
        };
        let _ = dom.set_property(workspace, key, value);
    }
    let probe = child(&mut dom, "Folder", "S_Probe", workspace);
    for d in RING {
        part(&mut dom, &format!("P{d}"), probe, [*d, 5.0, 0.0]);
    }
    for d in AXES {
        let s = d / std::f32::consts::SQRT_2;
        part(&mut dom, &format!("N{d}"), probe, [-d, 5.0, 0.0]);
        part(&mut dom, &format!("Z{d}"), probe, [0.0, 5.0, *d]);
        part(&mut dom, &format!("D{d}"), probe, [s, 5.0, s]);
        part(&mut dom, &format!("H{d}"), probe, [0.0, *d, 0.0]);
    }
    // A long part whose middle is out of range but whose end isn't.
    let big = part(&mut dom, "Big1500", probe, [1500.0, 5.0, -40.0]);
    let _ = dom.set_property(
        big,
        "Size",
        Variant::Vector3(Vector3Data {
            x: 1000.0,
            y: 2.0,
            z: 2.0,
        }),
    );
    for (prefix, [x, z]) in [
        ("R", [-3000.0, 0.0]),
        ("T", [6000.0, 0.0]),
        ("Q", [0.0, 6000.0]),
        ("F", [0.0, -6000.0]),
    ] {
        for o in CLUSTER {
            part(&mut dom, &format!("{prefix}{o}"), probe, [x, 5.0, z + o]);
        }
    }
    let focus = part(&mut dom, "Focus", workspace, [0.0, 5.0, -6000.0]);
    let _ = dom.set_property(focus, "Transparency", Variant::Float32(1.0));
    let pad = part(&mut dom, "TPad", workspace, [6000.0, -1.0, 0.0]);
    let _ = dom.set_property(
        pad,
        "Size",
        Variant::Vector3(Vector3Data {
            x: 40.0,
            y: 2.0,
            z: 40.0,
        }),
    );

    model(
        &mut dom,
        "Atomic1",
        1,
        &[[900.0, 5.0, 20.0], [1100.0, 5.0, 20.0], [1300.0, 5.0, 20.0]],
        workspace,
    );
    model(
        &mut dom,
        "Atomic2",
        1,
        &[[1600.0, 5.0, 20.0], [1700.0, 5.0, 20.0]],
        workspace,
    );
    model(
        &mut dom,
        "AtomicNear",
        1,
        &[[40.0, 5.0, 20.0], [3000.0, 5.0, 20.0]],
        workspace,
    );
    model(
        &mut dom,
        "Persist",
        2,
        &[[4000.0, 5.0, 40.0], [4010.0, 5.0, 40.0]],
        workspace,
    );
    model(&mut dom, "PerPlayer", 3, &[[4000.0, 5.0, 60.0]], workspace);
    model(&mut dom, "PerOther", 3, &[[4000.0, 5.0, 80.0]], workspace);
    let default = model(
        &mut dom,
        "DefaultM",
        0,
        &[[3000.0, 5.0, 40.0], [3010.0, 5.0, 40.0]],
        workspace,
    );
    let first = dom.get(default).and_then(|m| m.children().first().copied());
    if let Some(first) = first {
        let _ = dom.set_property(default, "PrimaryPart", Variant::Ref(first));
    }
    let info = child(&mut dom, "Folder", "S_DefaultInfo", default);
    child(&mut dom, "StringValue", "S_DefaultValue", info);
    model(&mut dom, "NonatomicM", 4, &[[3000.0, 5.0, 60.0]], workspace);
    let folder = child(&mut dom, "Folder", "S_FarFolder", workspace);
    child(&mut dom, "StringValue", "S_FarValue", folder);
    part(&mut dom, "FarFolderPart", folder, [3000.0, 5.0, 80.0]);
    // Parts inside parts: does the child stream with its parent?
    let outer = part(&mut dom, "NestOuterFar", workspace, [3000.0, 5.0, 100.0]);
    part(&mut dom, "NestInnerNear", outer, [10.0, 5.0, 100.0]);
    child(&mut dom, "Decal", "S_FarDecal", outer);
    let outer = part(&mut dom, "NestOuterNear", workspace, [10.0, 5.0, 120.0]);
    part(&mut dom, "NestInnerFar", outer, [3000.0, 5.0, 120.0]);
    // Welded assemblies reaching out of range: an anchored one, and a
    // loose one resting on the baseplate and a far shelf.
    for (name, z, anchored) in [("WeldA", 140.0, true), ("WeldU", 160.0, false)] {
        let y = if anchored { 5.0 } else { 1.0 };
        let near = part(&mut dom, &format!("{name}Near"), workspace, [12.0, y, z]);
        let far = part(&mut dom, &format!("{name}Far"), workspace, [3000.0, y, z]);
        let weld = child(&mut dom, "WeldConstraint", &format!("S_{name}"), near);
        let _ = dom.set_property(weld, "Part0", Variant::Ref(near));
        let _ = dom.set_property(weld, "Part1", Variant::Ref(far));
        if !anchored {
            let _ = dom.set_property(far, "Anchored", Variant::Bool(false));
            let _ = dom.set_property(near, "Anchored", Variant::Bool(false));
            let shelf = part(&mut dom, "Shelf", workspace, [3000.0, -1.0, z]);
            let _ = dom.set_property(
                shelf,
                "Size",
                Variant::Vector3(Vector3Data {
                    x: 10.0,
                    y: 2.0,
                    z: 10.0,
                }),
            );
        }
    }

    let storage = service(&mut dom, "ReplicatedStorage");
    for (name, target) in [("S_FarRef", "S_P3000"), ("S_NearRef", "S_P16")] {
        let value = child(&mut dom, "ObjectValue", name, storage);
        let found = dom.get(probe).and_then(|p| {
            p.children()
                .iter()
                .copied()
                .find(|c| dom.get(*c).is_some_and(|c| c.name() == target))
        });
        if let Some(found) = found {
            let _ = dom.set_property(value, "Value", Variant::Ref(found));
        }
    }
    child(&mut dom, "RemoteFunction", "S_Call", storage);
    child(&mut dom, "RemoteEvent", "S_Ready", storage);
    let source = |dom: &mut WeakDom, script: Ref, text: &str| {
        let _ = dom.set_property(script, "Source", Variant::String(text.into()));
    };
    let first = service(&mut dom, "ReplicatedFirst");
    let script = child(&mut dom, "LocalScript", "StreamFirst", first);
    source(&mut dom, script, FIRST);
    let starter = service(&mut dom, "StarterPlayer");
    let scripts = dom
        .get(starter)
        .and_then(|s| s.children().first().copied())
        .unwrap_or(starter);
    let script = child(&mut dom, "LocalScript", "StreamClient", scripts);
    source(&mut dom, script, CLIENT);
    let sss = service(&mut dom, "ServerScriptService");
    let script = child(&mut dom, "Script", "StreamServer", sss);
    source(&mut dom, script, SERVER);
    dom.take_changes();
    Some(dom)
}

/// The probe's printed sections by name (`log` chunks joined), once it
/// printed [`DONE`].
pub fn parse(lines: &[String]) -> Option<BTreeMap<String, Value>> {
    if !lines.iter().any(|l| l == DONE) {
        return None;
    }
    let mut out: BTreeMap<String, Value> = BTreeMap::new();
    for line in lines {
        let Some((section, json)) = line.strip_prefix(PREFIX).and_then(|l| l.split_once(' '))
        else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(json) else {
            continue;
        };
        match (out.get_mut(section), value) {
            (Some(Value::Array(have)), Value::Array(more)) if section == "log" => have.extend(more),
            (_, value) => {
                out.insert(section.to_owned(), value);
            }
        }
    }
    Some(out)
}

/// Runs variant `name`'s probe in a [`Session`] with one player, for up to
/// `limit` s of game time.
pub fn run_in_sim(name: &str, limit: f64) -> Result<BTreeMap<String, Value>, String> {
    let dom = place(name).ok_or("no such variant")?;
    let mut session = Session::new(dom)?;
    session.set_script_timeout(std::time::Duration::from_secs(86_400));
    session.start();
    session.join("Player1", -1, None)?;
    let joined = session.time();
    let mut lines = Vec::new();
    let mut warnings = Vec::new();
    while session.time() < limit {
        session.step();
        for (peer, output) in session.take_output() {
            for (line, kind) in output.lines().iter().zip(output.kinds()) {
                if std::env::var_os("STREAMING_VERBOSE").is_some() {
                    eprintln!("{peer:?} {:.2}: {line}", session.time());
                }
                if peer != Peer::Server
                    && *kind == crate::LineKind::Warn
                    && line.contains("Infinite yield")
                {
                    let at = session.time() - joined;
                    warnings.push(serde_json::json!([at, line, "MessageWarning"]));
                }
            }
            if peer == Peer::Server {
                lines.extend(output.lines().iter().cloned());
            }
        }
        if let Some(mut sections) = parse(&lines) {
            sections.insert("warnings".into(), Value::Array(warnings));
            return Ok(sections);
        }
    }
    Err(format!(
        "the probe never finished; it printed:\n{}",
        lines.join("\n")
    ))
}

pub fn golden_path(name: &str) -> std::path::PathBuf {
    super::goldens_dir()
        .join("streaming")
        .join(format!("{name}.json"))
}
