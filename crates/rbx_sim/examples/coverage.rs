//! Which of the API dump's classes and members a Local Run script can
//! use, found by asking: a server script in a fresh [`Simulation`] gets an
//! instance of each class (`Instance.new`, `GetService`, or the first
//! creatable subclass for an abstract one) and indexes every member a game
//! script may touch. A member counts as there when indexing it gives a
//! value instead of an error or nil.
//!
//! ```text
//! cargo run -p rbx_sim --example coverage -- docs/research/local-run/COVERAGE.md
//! ```

use std::collections::{BTreeMap, BTreeSet};

use rbx_dom::{Variant, WeakDom};
use rbx_sim::Simulation;
use serde_json::Value;

const PREFIX: &str = "COV ";
const KINDS: [&str; 3] = ["Function", "Event", "Property"];

struct Class {
    name: String,
    superclass: String,
    tags: Vec<String>,
    /// (member type, name), the members a game script may use.
    members: Vec<(String, String)>,
}

fn tags(value: &Value) -> Vec<String> {
    value["Tags"]
        .as_array()
        .map(|t| {
            t.iter()
                .filter_map(|t| t.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn scriptable(member: &Value) -> bool {
    let security = match &member["Security"] {
        Value::Object(s) => s["Read"].as_str().unwrap_or("None").to_owned(),
        other => other.as_str().unwrap_or("None").to_owned(),
    };
    let tags = tags(member);
    security == "None"
        && KINDS.contains(&member["MemberType"].as_str().unwrap_or(""))
        && !["Deprecated", "NotScriptable", "Hidden"]
            .iter()
            .any(|t| tags.iter().any(|x| x == t))
}

fn load_dump() -> Vec<Class> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/API-Dump.json");
    let text = std::fs::read_to_string(path).expect("assets/API-Dump.json");
    let dump: Value = serde_json::from_str(&text).expect("the API dump is JSON");
    dump["Classes"]
        .as_array()
        .expect("Classes")
        .iter()
        .map(|c| Class {
            name: c["Name"].as_str().unwrap_or_default().to_owned(),
            superclass: c["Superclass"].as_str().unwrap_or_default().to_owned(),
            tags: tags(c),
            members: c["Members"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|m| scriptable(m))
                .map(|m| {
                    (
                        m["MemberType"].as_str().unwrap_or_default().to_owned(),
                        m["Name"].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect(),
        })
        .filter(|c| {
            !c.tags
                .iter()
                .any(|t| t == "Deprecated" || t == "NotScriptable")
        })
        .collect()
}

/// How a script reaches an instance of `class` (itself or a subclass):
/// `(class to make, is a service)`.
fn reach(classes: &[Class], class: &Class) -> Option<(String, bool)> {
    if class.tags.iter().any(|t| t == "Service") {
        return Some((class.name.clone(), true));
    }
    if !class.tags.iter().any(|t| t == "NotCreatable") {
        return Some((class.name.clone(), false));
    }
    classes
        .iter()
        .filter(|c| c.superclass == class.name)
        .find_map(|c| reach(classes, c))
}

fn lua_string(text: &str) -> String {
    format!("{text:?}")
}

fn probe_source(classes: &[Class]) -> String {
    let mut source = String::from(
        r#"local function probe(declared, class, service, members)
	local ok, instance = pcall(function()
		if service then
			return game:GetService(class)
		end
		return Instance.new(class)
	end)
	for _, member in members do
		local there = false
		if ok and instance ~= nil then
			local found, value = pcall(function()
				return instance[member]
			end)
			there = found and value ~= nil
		end
		print("COV " .. declared .. " " .. member .. " " .. (there and "1" or "0"))
	end
end
"#,
    );
    for class in classes.iter().filter(|c| !c.members.is_empty()) {
        let (made, service) = reach(classes, class).unwrap_or_default();
        let names: Vec<String> = class.members.iter().map(|(_, n)| lua_string(n)).collect();
        source.push_str(&format!(
            "probe({}, {}, {service}, {{ {} }})\n",
            lua_string(&class.name),
            lua_string(&made),
            names.join(", ")
        ));
    }
    source
}

fn probe(classes: &[Class]) -> BTreeSet<(String, String)> {
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    let sss = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let script = dom.new_instance("Script", "Coverage", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(probe_source(classes)));
    dom.take_changes();
    let mut sim = Simulation::new(dom).expect("the sim starts");
    // As Play (local) runs it: server and client in one, so the client-only
    // services (UserInputService, ContextActionService) count too.
    sim.set_client();
    sim.start();
    let output = sim.take_output();
    output
        .lines()
        .iter()
        .filter_map(|line| line.strip_prefix(PREFIX))
        .filter_map(|rest| {
            let mut words = rest.split(' ');
            let (class, member, there) = (words.next()?, words.next()?, words.next()?);
            (there == "1").then(|| (class.to_owned(), member.to_owned()))
        })
        .collect()
}

fn main() {
    let out = std::env::args().nth(1);
    let classes = load_dump();
    let there = probe(&classes);

    let mut totals: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut rows = String::new();
    let mut untouched = Vec::new();
    for class in classes.iter().filter(|c| !c.members.is_empty()) {
        let mut counts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        let (mut have, mut missing) = (Vec::new(), Vec::new());
        for (kind, name) in &class.members {
            let ok = there.contains(&(class.name.clone(), name.clone()));
            let entry = counts.entry(kind.as_str()).or_default();
            entry.1 += 1;
            let total = totals.entry(kind.as_str()).or_default();
            total.1 += 1;
            if ok {
                entry.0 += 1;
                total.0 += 1;
            }
            if kind != "Property" {
                if ok { &mut have } else { &mut missing }.push(name.as_str());
            }
        }
        if counts.values().all(|c| c.0 == 0) {
            untouched.push(class.name.as_str());
            continue;
        }
        let cell = |kind: &str| {
            counts
                .get(kind)
                .map_or("-".to_owned(), |(a, b)| format!("{a}/{b}"))
        };
        rows.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            class.name,
            cell("Property"),
            cell("Function"),
            cell("Event"),
            have.join(", "),
            missing.join(", ")
        ));
    }

    let summary: Vec<String> = KINDS
        .iter()
        .map(|kind| {
            let (a, b) = totals.get(kind).copied().unwrap_or_default();
            format!(
                "| {kind} | {a} | {b} | {:.0}% |",
                100.0 * a as f64 / b.max(1) as f64
            )
        })
        .collect();
    let text = format!(
        "# Local Run coverage\n\n\
         Generated by `cargo run -p rbx_sim --example coverage -- docs/research/local-run/COVERAGE.md` from \
         `assets/API-Dump.json`. Only members a game script may touch count (security `None`, not deprecated, \
         hidden or `NotScriptable`), each under the class that declares it. A server script in a fresh Local Run \
         makes an instance of each class (or of its first creatable subclass) and indexes every member; it counts \
         as there when that gives a value, not an error or nil.\n\n\
         Read the property numbers with care: any property the reflection database knows reads back as data \
         (its saved value or default), whether or not the runtime acts on it. What the sim actually simulates is \
         listed in `PLAN.md`. Functions and events count when they exist, some of them as documented no-ops on a \
         lone server (`SetNetworkOwner`).\n\n\
         | Member type | There | Total | |\n|---|---|---|---|\n{}\n\n\
         ## By class\n\n\
         Classes with at least one member there. Properties, functions and events as there/total, then the \
         functions and events by name.\n\n\
         | Class | Properties | Functions | Events | Functions and events there | Not there |\n\
         |---|---|---|---|---|---|\n{rows}\n\
         ## Nothing there yet\n\n{} classes: {}.\n",
        summary.join("\n"),
        untouched.len(),
        untouched.join(", ")
    );
    for line in &summary {
        println!("{line}");
    }
    println!("{} classes with nothing there", untouched.len());
    if let Some(path) = out {
        std::fs::write(&path, text).unwrap_or_else(|err| panic!("{path}: {err}"));
    }
}
