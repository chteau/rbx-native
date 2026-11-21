//! Which of the API dump's classes and members rbx-native's scripts can
//! use, by security level, found by asking each place a script runs:
//!
//! - game scripts (security `None`): a script in a fresh [`Simulation`];
//! - plugins (`PluginSecurity`): a plugin in the plugin host;
//! - the Command Bar (`LocalUserSecurity`, plus `PluginSecurity`): the
//!   Command Bar's runtime.
//!
//! Each probe gets an instance of every class (`Instance.new`, a service,
//! a factory for the classes only another API hands out, or the first
//! creatable subclass for an abstract one) and indexes every member of the
//! levels it can reach. A member counts as there when indexing it gives a
//! value instead of an error or nil.
//!
//! ```text
//! cargo run -p rbx_sim --example coverage -- docs/research/local-run/COVERAGE.md
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rbx_dom::{Variant, WeakDom};
use rbx_lua::plugin::{Context, Input, Origin, OutputKind, PluginHost, PluginSource, SourceBody};
use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;
use rbx_sim::Simulation;
use serde_json::Value;

const PREFIX: &str = "COV\t";
/// What real Studio's Command Bar reaches, from `--import-studio`.
const STUDIO_REACH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/research/local-run/studio-reach.tsv"
);
const KINDS: [&str; 3] = ["Function", "Event", "Property"];

/// The dump's security levels, lowest first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Level {
    None,
    Plugin,
    LocalUser,
    RobloxScript,
    Roblox,
}

impl Level {
    fn parse(text: &str) -> Level {
        match text {
            "PluginSecurity" => Level::Plugin,
            "LocalUserSecurity" => Level::LocalUser,
            "RobloxScriptSecurity" => Level::RobloxScript,
            "RobloxSecurity" | "NotAccessibleSecurity" => Level::Roblox,
            _ => Level::None,
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Level::None => "None",
            Level::Plugin => "PluginSecurity",
            Level::LocalUser => "LocalUserSecurity",
            Level::RobloxScript => "RobloxScriptSecurity",
            Level::Roblox => "RobloxSecurity",
        }
    }
}

struct Member {
    kind: String,
    name: String,
    level: Level,
    /// Deprecated, hidden or `NotScriptable`: counted on their own, outside
    /// the target.
    retired: bool,
}

struct Class {
    name: String,
    superclass: String,
    tags: Vec<String>,
    members: Vec<Member>,
}

/// Which runtime a probe runs in.
#[derive(Clone, Copy, PartialEq)]
enum Host {
    Game,
    Plugin,
    CommandBar,
    /// Real Studio's Command Bar (`--studio-probe`): every level Roblox's
    /// own scripts stop short of, to see which ones it refuses.
    Studio,
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

fn member(value: &Value) -> Option<Member> {
    let kind = value["MemberType"].as_str().unwrap_or("");
    if !KINDS.contains(&kind) {
        return None;
    }
    let security = match &value["Security"] {
        Value::Object(s) => s["Read"].as_str().unwrap_or("None"),
        other => other.as_str().unwrap_or("None"),
    };
    let tags = tags(value);
    Some(Member {
        kind: kind.to_owned(),
        name: value["Name"].as_str().unwrap_or_default().to_owned(),
        level: Level::parse(security),
        retired: ["Deprecated", "NotScriptable", "Hidden"]
            .iter()
            .any(|t| tags.iter().any(|x| x == t)),
    })
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
                .filter_map(member)
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
    // Not creatable and no service, but every place has one.
    if class.name == "Terrain" {
        return Some((class.name.clone(), false));
    }
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

/// The classes the probe's `FACTORIES` reach through a service, which a
/// server answers (DataStores refuse a client).
const FROM_SERVICES: &[&str] = &[
    "GlobalDataStore",
    "DataStore",
    "OrderedDataStore",
    "DataStoreKeyInfo",
    "Pages",
    "DataStoreKeyPages",
    "DataStoreVersionPages",
    "DataStoreListingPages",
    "DataStorePages",
    "DataStoreKey",
    "DataStoreObjectVersionInfo",
    "DataStoreInfo",
    "MemoryStoreHashMap",
    "MemoryStoreSortedMap",
    "MemoryStoreQueue",
    "MemoryStoreHashMapPages",
    "Path",
    "FriendPages",
];

const GAME_FACTORIES: &str = r#"-- Classes only a service hands out.
local DSS = game:GetService("DataStoreService")
local MSS = game:GetService("MemoryStoreService")
local function store()
	local ds = DSS:GetDataStore("coverage")
	ds:SetAsync("k", 1)
	return ds
end
local FACTORIES = {
	GlobalDataStore = store,
	DataStore = store,
	OrderedDataStore = function()
		return DSS:GetOrderedDataStore("coverage")
	end,
	DataStoreKeyInfo = function()
		return select(2, store():GetAsync("k"))
	end,
	Pages = function()
		return store():ListKeysAsync()
	end,
	DataStoreKeyPages = function()
		return store():ListKeysAsync()
	end,
	DataStoreVersionPages = function()
		return store():ListVersionsAsync("k")
	end,
	DataStoreListingPages = function()
		store()
		return DSS:ListDataStoresAsync()
	end,
	DataStorePages = function()
		return DSS:GetOrderedDataStore("coverage"):GetSortedAsync(true, 10)
	end,
	DataStoreKey = function()
		return store():ListKeysAsync():GetCurrentPage()[1]
	end,
	DataStoreObjectVersionInfo = function()
		return store():ListVersionsAsync("k"):GetCurrentPage()[1]
	end,
	DataStoreInfo = function()
		store()
		return DSS:ListDataStoresAsync():GetCurrentPage()[1]
	end,
	MemoryStoreHashMap = function()
		return MSS:GetHashMap("coverage")
	end,
	MemoryStoreSortedMap = function()
		return MSS:GetSortedMap("coverage")
	end,
	MemoryStoreQueue = function()
		return MSS:GetQueue("coverage")
	end,
	MemoryStoreHashMapPages = function()
		return MSS:GetHashMap("coverage"):ListItemsAsync(10)
	end,
	Path = function()
		return game:GetService("PathfindingService"):CreatePath()
	end,
	FriendPages = function()
		return game:GetService("Players"):GetFriendsAsync(1)
	end,
}
"#;

/// The Studio side's classes: `settings()` services, what `plugin` hands
/// out, and the Command Bar's own script document.
const STUDIO_FACTORIES: &str = r#"local FACTORIES = {}
for _, name in { "Studio", "DebugSettings", "PhysicsSettings", "RenderSettings", "NetworkSettings",
	"GameSettings", "LuaSettings", "TaskScheduler" } do
	FACTORIES[name] = function()
		return settings():GetService(name)
	end
end
FACTORIES.StudioTheme = function()
	return settings().Studio.Theme
end
FACTORIES.ScriptDocument = function()
	return game:GetService("ScriptEditorService"):GetScriptDocuments()[1]
end
FACTORIES.StatsItem = function()
	return game:GetService("Stats"):FindFirstChildWhichIsA("StatsItem", true)
end
FACTORIES.DebuggerManager = function()
	return DebuggerManager()
end
FACTORIES.PluginManager = function()
	return PluginManager()
end
-- The Command Bar has no `plugin`, but Studio's PluginManager makes one.
local plugin = plugin
	or select(2, pcall(function()
		return PluginManager():CreatePlugin()
	end))
if typeof(plugin) == "Instance" then
	local toolbar
	local function bar()
		toolbar = toolbar or plugin:CreateToolbar("Coverage")
		return toolbar
	end
	local widget
	local function dock()
		widget = widget
			or plugin:CreateDockWidgetPluginGui(
				"Coverage",
				DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Float, false, false, 200, 100, 100, 50)
			)
		return widget
	end
	FACTORIES.Plugin = function()
		return plugin
	end
	FACTORIES.PluginToolbar = bar
	FACTORIES.PluginToolbarButton = function()
		return bar():CreateButton("Coverage", "", "")
	end
	FACTORIES.PluginMenu = function()
		return plugin:CreatePluginMenu("Coverage")
	end
	FACTORIES.PluginAction = function()
		return plugin:CreatePluginAction("Coverage", "Coverage", "")
	end
	FACTORIES.PluginGui = dock
	FACTORIES.DockWidgetPluginGui = dock
	FACTORIES.PluginMouse = function()
		return plugin:GetMouse()
	end
end
"#;

const PROBE: &str = r#"-- Every script reaches the data model itself as `game`.
FACTORIES.DataModel = function()
	return game
end
FACTORIES.ServiceProvider = FACTORIES.DataModel
local function probe(declared, class, service, members)
	local ok, instance = pcall(function()
		local factory = FACTORIES[declared]
		if factory then
			return factory()
		end
		if service then
			return game:GetService(class)
		end
		if class == "Terrain" then
			return workspace.Terrain
		end
		return Instance.new(class)
	end)
	for _, member in members do
		-- After the verdict: what the read gave, or why it failed.
		local there, detail = false, "no instance: " .. tostring(instance)
		if ok and instance ~= nil then
			local found, value = pcall(function()
				return instance[member]
			end)
			there = found and value ~= nil
			detail = if found then typeof(value) .. " " .. tostring(value) else tostring(value)
		end
		detail = string.sub((string.gsub(detail, "%s+", " ")), 1, 160)
		print(table.concat({ "COV", declared, member, there and "1" or "0", detail }, "\t"))
	end
end
"#;

/// The security levels `host` can reach.
fn levels(host: Host) -> &'static [Level] {
    match host {
        Host::Game => &[Level::None],
        Host::Plugin => &[Level::None, Level::Plugin],
        Host::CommandBar => &[Level::Plugin, Level::LocalUser],
        Host::Studio => &[Level::Plugin, Level::LocalUser, Level::RobloxScript],
    }
}

/// `server` picks the game probe's half: the classes only a server's
/// services hand out, or the rest.
fn probe_source(classes: &[Class], host: Host, server: bool) -> String {
    let mut source = String::from(match host {
        Host::Game => GAME_FACTORIES,
        _ => STUDIO_FACTORIES,
    });
    source.push_str(PROBE);
    for class in classes {
        if host == Host::Game && FROM_SERVICES.contains(&class.name.as_str()) != server {
            continue;
        }
        let names: Vec<String> = class
            .members
            .iter()
            .filter(|m| levels(host).contains(&m.level))
            .map(|m| lua_string(&m.name))
            .collect();
        if names.is_empty() {
            continue;
        }
        let (made, service) = reach(classes, class).unwrap_or_default();
        source.push_str(&format!(
            "probe({}, {}, {service}, {{ {} }})\n",
            lua_string(&class.name),
            lua_string(&made),
            names.join(", ")
        ));
    }
    source.push_str("print(\"COV_DONE\")\n");
    source
}

fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    dom.new_instance("Terrain", "Terrain", Some(workspace));
    dom.take_changes();
    dom
}

fn parse(lines: &[String]) -> BTreeSet<(String, String)> {
    lines
        .iter()
        .filter_map(|line| line.strip_prefix(PREFIX))
        .filter_map(|rest| {
            let mut words = rest.split('\t');
            let (class, member, there) = (words.next()?, words.next()?, words.next()?);
            (there == "1").then(|| (class.to_owned(), member.to_owned()))
        })
        .collect()
}

fn probe_game(classes: &[Class], server: bool) -> BTreeSet<(String, String)> {
    let mut dom = place();
    let sss = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let script = dom.new_instance("Script", "Coverage", Some(sss));
    let _ = dom.set_property(
        script,
        "Source",
        Variant::String(probe_source(classes, Host::Game, server)),
    );
    dom.take_changes();
    let mut sim = Simulation::new(dom).expect("the sim starts");
    // As Play (local) runs it: server and client in one, so the client-only
    // services (UserInputService, ContextActionService) count too; the
    // classes services hand out are asked of a server.
    if !server {
        sim.set_client();
    }
    sim.start();
    let mut lines = Vec::new();
    for _ in 0..600 {
        lines.extend(sim.take_output().lines().iter().cloned());
        if lines.iter().any(|l| l == "COV_DONE") {
            break;
        }
        sim.step();
    }
    parse(&lines)
}

fn probe_plugin(classes: &[Class]) -> BTreeSet<(String, String)> {
    let mut dom = place();
    let mut host = PluginHost::new(ReflectionDatabase::embedded(), None);
    host.add(PluginSource {
        name: "Coverage".to_owned(),
        body: SourceBody::Luau(probe_source(classes, Host::Plugin, false)),
        origin: Origin::Roblox {
            path: PathBuf::from("Coverage.lua"),
        },
    });
    let mut lines = Vec::new();
    let mut input = Input::Start;
    for tick in 0..600 {
        let context = Context {
            now: 100.0 + tick as f64 / 60.0,
            ..Context::default()
        };
        let outcome = host.dispatch(&mut dom, context, input);
        lines.extend(
            outcome
                .output
                .into_iter()
                .filter(|l| l.kind == OutputKind::Print)
                .map(|l| l.text),
        );
        if lines.iter().any(|l| l == "COV_DONE") {
            break;
        }
        input = Input::Tick;
    }
    parse(&lines)
}

fn probe_command_bar(classes: &[Class]) -> BTreeSet<(String, String)> {
    let mut runtime = Runtime::new(place(), ReflectionDatabase::embedded()).expect("runtime");
    let output = runtime
        .run(&probe_source(classes, Host::CommandBar, false))
        .unwrap_or_else(|err| panic!("the Command Bar probe failed: {err:?}"));
    parse(output.lines())
}

/// There/total per member type, for the members `wanted` picks.
type Counts = BTreeMap<&'static str, (usize, usize)>;

fn count(counts: &mut Counts, kind: &str, ok: bool) {
    let kind = KINDS.into_iter().find(|k| *k == kind).unwrap_or("Property");
    let entry = counts.entry(kind).or_default();
    entry.1 += 1;
    entry.0 += usize::from(ok);
}

/// A section's per-class table and its classes with nothing there.
/// `studio`, when given, also lists the missing properties and marks with
/// † the missing members real Studio doesn't reach either.
fn section(
    classes: &[Class],
    there: &BTreeSet<(String, String)>,
    wanted: impl Fn(&Member) -> bool,
    studio: Option<&BTreeSet<(String, String)>>,
) -> (Counts, String, Vec<String>) {
    let mut totals = Counts::new();
    let mut rows = String::new();
    let mut untouched = Vec::new();
    for class in classes {
        let mut counts = Counts::new();
        let (mut have, mut missing) = (Vec::new(), Vec::new());
        for member in class.members.iter().filter(|m| wanted(m)) {
            let key = (class.name.clone(), member.name.clone());
            let ok = there.contains(&key);
            count(&mut counts, &member.kind, ok);
            count(&mut totals, &member.kind, ok);
            if ok && member.kind != "Property" {
                have.push(member.name.clone());
            } else if !ok && (member.kind != "Property" || studio.is_some()) {
                let absent = studio.is_some_and(|s| !s.contains(&key));
                missing.push(format!("{}{}", member.name, if absent { "†" } else { "" }));
            }
        }
        if counts.is_empty() {
            continue;
        }
        if counts.values().all(|c| c.0 == 0) {
            untouched.push(class.name.clone());
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
    (totals, rows, untouched)
}

fn ratio((a, b): (usize, usize)) -> String {
    if b == 0 {
        return "-".to_owned();
    }
    format!("{a}/{b} ({:.0}%)", 100.0 * a as f64 / b.max(1) as f64)
}

/// One summary row: each member type, then all of them.
fn summary_row(label: &str, level: &str, counts: &Counts) -> String {
    let cells: Vec<String> = KINDS
        .iter()
        .map(|k| ratio(counts.get(k).copied().unwrap_or_default()))
        .collect();
    let all = counts
        .values()
        .fold((0, 0), |(a, b), (x, y)| (a + x, b + y));
    format!(
        "| {label} | `{level}` | {} | {} |",
        cells.join(" | "),
        ratio(all)
    )
}

fn by_class(title: &str, intro: &str, rows: &str, untouched: &[String]) -> String {
    format!(
        "## {title}\n\n{intro} Properties, functions and events as there/total, then the functions and \
         events by name; classes with nothing there are listed after.\n\n\
         | Class | Properties | Functions | Events | Functions and events there | Not there |\n\
         |---|---|---|---|---|---|\n{rows}\n\
         Nothing there yet, {} classes: {}.\n\n",
        untouched.len(),
        untouched.join(", ")
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let classes = load_dump();
    // The same probe for real Studio's Command Bar: its printed `COV` lines
    // (grep them from Studio's log) are what this tool's are checked against.
    match args.as_slice() {
        [flag, path] if flag == "--studio-probe" => {
            let source = probe_source(&classes, Host::Studio, false);
            std::fs::write(path, source).unwrap_or_else(|err| panic!("{path}: {err}"));
            return;
        }
        // Studio's log after running the probe: what it reached, for the
        // plugin and Command Bar levels, kept as `studio-reach.tsv`.
        [flag, log] if flag == "--import-studio" => {
            let text = std::fs::read_to_string(log).unwrap_or_else(|err| panic!("{log}: {err}"));
            let lines: Vec<String> = text
                .lines()
                .filter_map(|l| l.find(PREFIX).map(|at| l[at..].to_owned()))
                .collect();
            let there = parse(&lines);
            let mut out = String::new();
            for class in &classes {
                for m in &class.members {
                    if matches!(m.level, Level::Plugin | Level::LocalUser) {
                        let key = (class.name.clone(), m.name.clone());
                        let ok = u8::from(there.contains(&key));
                        out.push_str(&format!("{}\t{}\t{ok}\n", class.name, m.name));
                    }
                }
            }
            std::fs::write(STUDIO_REACH, out).expect("studio-reach.tsv");
            return;
        }
        _ => {}
    }
    let studio: Option<BTreeSet<(String, String)>> =
        std::fs::read_to_string(STUDIO_REACH).ok().map(|text| {
            text.lines()
                .filter_map(|l| {
                    let mut words = l.split('\t');
                    let (class, member, there) = (words.next()?, words.next()?, words.next()?);
                    (there == "1").then(|| (class.to_owned(), member.to_owned()))
                })
                .collect()
        });
    let out = args.first().cloned();
    let mut game = probe_game(&classes, false);
    game.extend(probe_game(&classes, true));
    let plugin = probe_plugin(&classes);
    let command_bar = probe_command_bar(&classes);

    let at = |level: Level| move |m: &Member| m.level == level && !m.retired;
    let retired = |level: Level| move |m: &Member| m.level == level && m.retired;
    let s = studio.as_ref();
    let (game_totals, game_rows, game_none) = section(&classes, &game, at(Level::None), None);
    let (plugin_totals, plugin_rows, plugin_none) =
        section(&classes, &plugin, at(Level::Plugin), s);
    let (bar_totals, bar_rows, bar_none) = section(&classes, &command_bar, at(Level::LocalUser), s);
    let mut target = vec![
        summary_row("Game scripts (Local Run)", "None", &game_totals),
        summary_row("Plugins (plugin host)", "PluginSecurity", &plugin_totals),
        summary_row("Command Bar", "LocalUserSecurity", &bar_totals),
    ];
    if let Some(studio) = s {
        target.push(summary_row(
            "Real Studio's Command Bar",
            "PluginSecurity",
            &section(&classes, studio, at(Level::Plugin), None).0,
        ));
        target.push(summary_row(
            "Real Studio's Command Bar",
            "LocalUserSecurity",
            &section(&classes, studio, at(Level::LocalUser), None).0,
        ));
    }
    let also = [
        summary_row(
            "Plugins: the game API in the plugin host",
            "None",
            &section(&classes, &plugin, at(Level::None), None).0,
        ),
        summary_row(
            "Command Bar: the plugin API",
            "PluginSecurity",
            &section(&classes, &command_bar, at(Level::Plugin), None).0,
        ),
    ];
    let retired_rows = [
        summary_row(
            "Game scripts (Local Run)",
            "None",
            &section(&classes, &game, retired(Level::None), None).0,
        ),
        summary_row(
            "Plugins (plugin host)",
            "PluginSecurity",
            &section(&classes, &plugin, retired(Level::Plugin), None).0,
        ),
        summary_row(
            "Command Bar",
            "LocalUserSecurity",
            &section(&classes, &command_bar, retired(Level::LocalUser), None).0,
        ),
    ];
    let unreachable: Vec<String> = [Level::RobloxScript, Level::Roblox]
        .into_iter()
        .map(|level| {
            let mut counts = Counts::new();
            for m in classes.iter().flat_map(|c| &c.members) {
                if m.level == level && !m.retired {
                    count(&mut counts, &m.kind, false);
                }
            }
            let cells: Vec<String> = KINDS
                .iter()
                .map(|k| counts.get(k).map_or(0, |c| c.1).to_string())
                .collect();
            format!("| `{}` | {} |", level.tag(), cells.join(" | "))
        })
        .collect();

    let header =
        "| Where | Security | Functions | Events | Properties | All |\n|---|---|---|---|---|---|";
    let text = format!(
        "# Local Run coverage\n\n\
         Generated by `cargo run -p rbx_sim --example coverage -- docs/research/local-run/COVERAGE.md` from \
         `assets/API-Dump.json`, by security level. Each member counts once, under the class that declares it. \
         Each place a script runs gets a probe: a server script in a fresh Local Run for game scripts (security \
         `None`), a plugin in the plugin host for plugins (`PluginSecurity`), and a Command Bar run for \
         `LocalUserSecurity`. The probe makes an instance of each class (or of its first creatable subclass, or \
         asks the API that hands one out) and indexes every member; it counts as there when that gives a value, \
         not an error or nil.\n\n\
         The same probe ran in real Studio 0.748's Command Bar (`--studio-probe`, then `--import-studio` on \
         Studio's log, kept in `studio-reach.tsv`). The Command Bar runs at identity 4 and reaches every \
         `PluginSecurity` and `LocalUserSecurity` member whose instance a script can get (`PluginManager():\
         CreatePlugin()` hands it a `Plugin`); the member lists below mark with † what Studio didn't reach \
         either (`IXPService` isn't a service there, `File`, `PluginConnection` and the like aren't handed out).\n\n\
         Read the property numbers with care: any property the reflection database knows reads back as data \
         (its saved value or default), whether or not the runtime acts on it. What the sim actually simulates is \
         listed in `PLAN.md`. Functions and events count when they exist, whether or not the runtime acts on \
         them.\n\n\
         ## Target\n\n\
         Members that aren't deprecated, hidden or `NotScriptable`.\n\n{header}\n{}\n\n\
         Also measured: a plugin uses the game API too, and the Command Bar the plugin API.\n\n{header}\n{}\n\n\
         Plugins during Play: real Studio loads every plugin again into Play's server and client data models. \
         Local Run hosts no plugins yet, so none of the plugin API is there during a Local Run.\n\n\
         ## Deprecated, hidden and `NotScriptable`\n\n\
         Counted on their own, outside the target.\n\n{header}\n{}\n\n\
         ## Out of reach\n\n\
         Neither plugins nor the Command Bar may use these in Studio (they're for Roblox's own scripts and \
         built-in plugins), so they aren't counted. In Studio's Command Bar a `RobloxScriptSecurity` property \
         read fails with \"The current thread cannot read 'X' (lacking capability RobloxScript)\"; its functions \
         and events index fine, and a call fails with \"The current thread cannot call 'X' (lacking capability \
         RobloxScript)\". Members by type:\n\n\
         | Security | Functions | Events | Properties |\n|---|---|---|---|\n{}\n\n{}{}{}",
        target.join("\n"),
        also.join("\n"),
        retired_rows.join("\n"),
        unreachable.join("\n"),
        by_class(
            "Game scripts by class",
            "Security `None`, measured in Local Run.",
            &game_rows,
            &game_none
        ),
        by_class(
            "Plugins by class",
            "Security `PluginSecurity`, measured in the plugin host.",
            &plugin_rows,
            &plugin_none
        ),
        by_class(
            "Command Bar by class",
            "Security `LocalUserSecurity`, measured in the Command Bar.",
            &bar_rows,
            &bar_none
        ),
    );
    for line in target.iter().chain(&also).chain(&retired_rows) {
        println!("{line}");
    }
    if let Some(path) = out {
        std::fs::write(&path, text.trim_end().to_owned() + "\n")
            .unwrap_or_else(|err| panic!("{path}: {err}"));
    }
}
