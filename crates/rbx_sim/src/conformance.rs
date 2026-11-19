//! The conformance suite: Luau tests that print a JSON result, run the
//! same way in real Roblox (goldens, recorded by the `record_goldens`
//! example) and in [`Simulation`] (the `conformance` test and example).
//!
//! The tests live in `conformance/tests/` and the goldens in
//! `conformance/goldens/`, one JSON file per test.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use serde_json::Value;

use crate::Simulation;

/// What a test's printed line starts with, and the runner's last line.
pub const PREFIX: &str = "CONFORMANCE ";
pub const DONE: &str = "CONFORMANCE_DONE";
/// The folder in `ServerScriptService` the tests are modules of.
pub const FOLDER: &str = "Conformance";
pub const RUNNER: &str = "ConformanceRunner";

/// Seconds the runner waits before the first test, so a real server is
/// past its start-up hitches.
const SETTLE: f64 = 3.0;

/// Passes over the suite in one run. Roblox isn't deterministic (the
/// first pass on a fresh server also starts from a different point in the
/// frame), so goldens keep the median and the spread, and rbx_sim runs the
/// same passes to compare like for like.
pub const PASSES: u32 = 5;

pub struct Test {
    pub name: &'static str,
    pub source: &'static str,
    /// Absolute tolerance per result field; `"*"` for the rest.
    pub tolerance: &'static [(&'static str, f64)],
}

impl Test {
    pub fn tolerance_of(&self, field: &str) -> f64 {
        let find = |key: &str| self.tolerance.iter().find(|(k, _)| *k == key).map(|t| t.1);
        find(field).or_else(|| find("*")).unwrap_or(0.0)
    }
}

const HARNESS: &str = include_str!("../conformance/tests/Harness.luau");

pub const TESTS: &[Test] = &[
    Test {
        name: "FreeFall",
        source: include_str!("../conformance/tests/FreeFall.luau"),
        tolerance: &[("*", 0.5)],
    },
    Test {
        name: "RestingContact",
        source: include_str!("../conformance/tests/RestingContact.luau"),
        tolerance: &[("*", 0.02)],
    },
    Test {
        name: "FrictionSlide",
        source: include_str!("../conformance/tests/FrictionSlide.luau"),
        tolerance: &[("*", 0.5), ("stopped_beat", 2.0)],
    },
    Test {
        name: "ElasticBounce",
        source: include_str!("../conformance/tests/ElasticBounce.luau"),
        tolerance: &[("*", 0.5), ("first_bounce_beat", 1.0)],
    },
    Test {
        name: "TouchedOrder",
        source: include_str!("../conformance/tests/TouchedOrder.luau"),
        tolerance: &[],
    },
    Test {
        name: "TaskTiming",
        source: include_str!("../conformance/tests/TaskTiming.luau"),
        tolerance: &[
            ("*", 1.0),
            ("wait0", 0.01),
            ("wait01", 0.02),
            ("wait05", 0.02),
            ("legacy_wait", 0.02),
        ],
    },
    Test {
        name: "RunServiceOrder",
        source: include_str!("../conformance/tests/RunServiceOrder.luau"),
        tolerance: &[],
    },
    Test {
        name: "SignalBehavior",
        source: include_str!("../conformance/tests/SignalBehavior.luau"),
        tolerance: &[],
    },
    Test {
        name: "FreeFallTrace",
        source: include_str!("../conformance/tests/FreeFallTrace.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "WaitRounding",
        source: include_str!("../conformance/tests/WaitRounding.luau"),
        tolerance: &[("*", 0.0)],
    },
    Test {
        name: "Raycast",
        source: include_str!("../conformance/tests/Raycast.luau"),
        tolerance: &[],
    },
    Test {
        name: "Shapecast",
        source: include_str!("../conformance/tests/Shapecast.luau"),
        tolerance: &[],
    },
    Test {
        name: "Overlap",
        source: include_str!("../conformance/tests/Overlap.luau"),
        tolerance: &[],
    },
    Test {
        name: "HingeFree",
        source: include_str!("../conformance/tests/HingeFree.luau"),
        tolerance: &[("*", 0.1), ("angle_30", 2.0)],
    },
    Test {
        name: "HingeMotor",
        source: include_str!("../conformance/tests/HingeMotor.luau"),
        tolerance: &[("*", 0.05), ("angle_10", 2.0), ("angle_30", 2.0)],
    },
    Test {
        name: "HingeServo",
        source: include_str!("../conformance/tests/HingeServo.luau"),
        tolerance: &[("*", 2.0)],
    },
    Test {
        name: "BallSocket",
        source: include_str!("../conformance/tests/BallSocket.luau"),
        tolerance: &[("*", 0.1)],
    },
    Test {
        name: "Rope",
        source: include_str!("../conformance/tests/Rope.luau"),
        tolerance: &[("*", 0.1)],
    },
    Test {
        name: "Spring",
        source: include_str!("../conformance/tests/Spring.luau"),
        tolerance: &[("*", 0.1)],
    },
    Test {
        name: "Prismatic",
        source: include_str!("../conformance/tests/Prismatic.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "AlignPosition",
        source: include_str!("../conformance/tests/AlignPosition.luau"),
        tolerance: &[("*", 0.1)],
    },
    Test {
        name: "AlignOrientation",
        source: include_str!("../conformance/tests/AlignOrientation.luau"),
        tolerance: &[("*", 2.0)],
    },
    Test {
        name: "LinearVelocity",
        source: include_str!("../conformance/tests/LinearVelocity.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "AngularVelocity",
        source: include_str!("../conformance/tests/AngularVelocity.luau"),
        tolerance: &[("*", 0.05), ("dy_30", 0.5)],
    },
    Test {
        name: "VectorForce",
        source: include_str!("../conformance/tests/VectorForce.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "Attachments",
        source: include_str!("../conformance/tests/Attachments.luau"),
        tolerance: &[],
    },
    Test {
        name: "BoxBounce",
        source: include_str!("../conformance/tests/BoxBounce.luau"),
        tolerance: &[("*", 0.3), ("first_bounce_beat", 1.0), ("bounces", 1.0)],
    },
    Test {
        name: "SleepWake",
        source: include_str!("../conformance/tests/SleepWake.luau"),
        tolerance: &[("*", 0.1), ("still_from", 10.0)],
    },
    Test {
        name: "BoxStack",
        source: include_str!("../conformance/tests/BoxStack.luau"),
        tolerance: &[("*", 0.02)],
    },
    Test {
        name: "Damping",
        source: include_str!("../conformance/tests/Damping.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "RampSlide",
        source: include_str!("../conformance/tests/RampSlide.luau"),
        tolerance: &[("*", 0.2)],
    },
    Test {
        name: "MeshCollision",
        source: include_str!("../conformance/tests/MeshCollision.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "FreeFallRates",
        source: include_str!("../conformance/tests/FreeFallRates.luau"),
        tolerance: &[("*", 1.0)],
    },
];

/// The tests named in `only` (all of them when it's empty).
pub fn selected(only: &[String]) -> impl Iterator<Item = (usize, &'static Test)> + '_ {
    TESTS
        .iter()
        .enumerate()
        .filter(move |(_, t)| only.is_empty() || only.iter().any(|n| n == t.name))
}

/// The runner `Script`: each test in `only` (every test when empty) in
/// turn, in a corner of its own that depends only on its place in
/// [`TESTS`], the whole lot `runs` times over.
pub fn runner_source(runs: u32, only: &[String]) -> String {
    let names: Vec<String> = selected(only)
        .map(|(i, t)| format!("{{ {}, \"{}\" }}", i + 1, t.name))
        .collect();
    format!(
        r#"local HttpService = game:GetService("HttpService")
local tests = script.Parent:WaitForChild("{FOLDER}")
local h = require(tests:WaitForChild("Harness"))
task.wait({SETTLE})
for _ = 1, {runs} do
	for _, entry in {{ {names} }} do
		local i, name = entry[1], entry[2]
		-- Every test starts just after a Heartbeat, whatever ran before it.
		h.beat()
		local ok, result = pcall(require(tests:WaitForChild(name)), h, Vector3.new(2000 + i * 300, 300, 2000))
		if not ok then
			result = {{ error = tostring(result) }}
		end
		print("{PREFIX}" .. name .. " " .. HttpService:JSONEncode(result))
	end
end
print("{DONE}")
"#,
        names = names.join(", ")
    )
}

fn child(dom: &mut WeakDom, class: &str, name: &str, parent: Ref) -> Ref {
    dom.new_instance(class, name, Some(parent))
}

fn service(dom: &mut WeakDom, class: &str) -> Ref {
    let found = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class));
    found.unwrap_or_else(|| dom.new_instance(class, class, None))
}

/// Adds the tests and a runner doing `runs` passes to
/// `ServerScriptService` in `dom`.
/// `ServerScriptService` in `dom`, only the tests in `only` when it isn't
/// empty.
pub fn add_to(dom: &mut WeakDom, runs: u32, only: &[String]) {
    let sss = service(dom, "ServerScriptService");
    let folder = child(dom, "Folder", FOLDER, sss);
    let modules =
        std::iter::once(("Harness", HARNESS)).chain(selected(only).map(|(_, t)| (t.name, t.source)));
    for (name, source) in modules {
        let module = child(dom, "ModuleScript", name, folder);
        let _ = dom.set_property(module, "Source", Variant::String(source.into()));
    }
    let runner = child(dom, "Script", RUNNER, sss);
    let _ = dom.set_property(runner, "Source", Variant::String(runner_source(runs, only)));
}

/// A bare place with the suite: a baseplate, a spawn, the tests.
pub fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = service(&mut dom, "Workspace");
    let base = child(&mut dom, "Part", "Baseplate", workspace);
    let _ = dom.set_property(base, "Anchored", Variant::Bool(true));
    let _ = dom.set_property(
        base,
        "Size",
        Variant::Vector3(Vector3Data {
            x: 512.0,
            y: 20.0,
            z: 512.0,
        }),
    );
    let _ = dom.set_property(
        base,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: -10.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    add_to(&mut dom, PASSES, &[]);
    dom.take_changes();
    dom
}

/// The results in `lines` (a runner's output), by test name, one per pass.
pub fn parse(lines: &[String]) -> BTreeMap<String, Vec<Value>> {
    let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for line in lines {
        let Some((name, json)) = line.strip_prefix(PREFIX).and_then(|r| r.split_once(' ')) else {
            continue;
        };
        if let Ok(value) = serde_json::from_str(json) {
            out.entry(name.to_owned()).or_default().push(value);
        }
    }
    out
}

/// Several passes of one test folded into one result and its spread: a
/// number field is the median, its spread `max - min`; a text field is the
/// most common value, its spread every distinct value when they differ.
pub fn aggregate(passes: &[Value]) -> (Value, Value) {
    let mut fields: Vec<&String> = passes
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|o| o.keys())
        .collect();
    fields.sort();
    fields.dedup();
    let (mut result, mut spread) = (serde_json::Map::new(), serde_json::Map::new());
    for field in fields {
        let values: Vec<&Value> = passes.iter().filter_map(|p| p.get(field)).collect();
        let mut numbers: Vec<f64> = values.iter().filter_map(|v| v.as_f64()).collect();
        if numbers.len() == values.len() && !numbers.is_empty() {
            numbers.sort_by(f64::total_cmp);
            let median = numbers[numbers.len() / 2];
            let range = numbers[numbers.len() - 1] - numbers[0];
            result.insert(field.clone(), serde_json::json!(median));
            spread.insert(field.clone(), serde_json::json!(range));
            continue;
        }
        let mut counts: Vec<(&Value, usize)> = Vec::new();
        for value in &values {
            match counts.iter_mut().find(|(v, _)| v == value) {
                Some((_, n)) => *n += 1,
                None => counts.push((value, 1)),
            }
        }
        counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        if let Some((common, _)) = counts.first() {
            result.insert(field.clone(), (*common).clone());
        }
        if counts.len() > 1 {
            let seen = counts.iter().map(|(v, _)| (*v).clone()).collect();
            spread.insert(field.clone(), Value::Array(seen));
        }
    }
    (Value::Object(result), Value::Object(spread))
}

/// Runs the suite in [`Simulation`], [`PASSES`] times like the goldens,
/// and folds the passes the same way ([`aggregate`]); `Err` with what it
/// printed when the runner never finished within `limit` s of game time.
pub fn run_in_sim(limit: f64) -> Result<BTreeMap<String, Value>, String> {
    let mut sim = Simulation::new(place())?;
    sim.start();
    let mut lines = Vec::new();
    while sim.time() < limit {
        sim.step();
        let output = sim.take_output();
        lines.extend(output.lines().iter().cloned());
        if lines.iter().any(|l| l == DONE) {
            return Ok(parse(&lines)
                .into_iter()
                .map(|(name, passes)| (name, aggregate(&passes).0))
                .collect());
        }
    }
    Err(format!(
        "the runner never finished; it printed:\n{}",
        lines.join("\n")
    ))
}

pub fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("conformance/goldens")
}

/// A golden as stored: `status` is `recorded` or `unrecorded`.
pub fn golden(name: &str) -> Option<Value> {
    let text = std::fs::read_to_string(goldens_dir().join(format!("{name}.json"))).ok()?;
    serde_json::from_str(&text).ok()
}

/// One result field, Roblox's against ours.
#[derive(Debug, Clone)]
pub struct Row {
    pub test: String,
    pub field: String,
    pub golden: String,
    pub ours: String,
    /// `ours - golden` for numbers.
    pub delta: Option<f64>,
    pub pass: bool,
}

fn show(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "-".to_owned(),
    }
}

/// Field by field, every field either side has. A number passes inside the
/// test's tolerance or Roblox's own spread across passes, whichever is
/// wider; text must match.
pub fn compare(test: &Test, golden: &Value, spread: &Value, ours: &Value) -> Vec<Row> {
    let empty = serde_json::Map::new();
    let golden = golden.as_object().unwrap_or(&empty);
    let ours = ours.as_object().unwrap_or(&empty);
    // `_` fields are traces for working things out, not results.
    let mut fields: Vec<&String> = golden
        .keys()
        .chain(ours.keys())
        .filter(|f| !f.starts_with('_'))
        .collect();
    fields.sort();
    fields.dedup();
    fields
        .into_iter()
        .map(|field| {
            let (g, o) = (golden.get(field), ours.get(field));
            let delta = match (g.and_then(Value::as_f64), o.and_then(Value::as_f64)) {
                (Some(g), Some(o)) => Some(o - g),
                _ => None,
            };
            let pass = match delta {
                Some(d) => {
                    let noise = spread.get(field).and_then(Value::as_f64).unwrap_or(0.0);
                    d.abs() <= test.tolerance_of(field).max(noise) + 1e-9
                }
                None => g.is_some() && g == o,
            };
            Row {
                test: test.name.to_owned(),
                field: field.clone(),
                golden: show(g),
                ours: show(o),
                delta,
                pass,
            }
        })
        .collect()
}
