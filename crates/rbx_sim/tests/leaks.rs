//! What the Leak finder dock reads off a run: scripted leaks of each kind
//! are suspects, and a script doing the same things without leaking is
//! not.

use rbx_dom::{Variant, WeakDom};
use rbx_lua::leaks::{self, Kind, Sample};
use rbx_sim::{conformance, Peer, Session};

/// Every tenth beat: a part parented to nil with a connection on it, kept
/// in a table that only grows.
const LEAKY: &str = r#"
local holder = {}
local n = 0
game:GetService("RunService").Heartbeat:Connect(function()
    n += 1
    if n % 10 == 0 then
        local part = Instance.new("Part")
        part.Touched:Connect(function() end)
        table.insert(holder, part)
    end
end)
"#;

/// The same work, no leak: a capped cache, one template its handler keeps,
/// a connection on a part in the place, parts destroyed or dropped.
const STEADY: &str = r#"
local cache = {}
local template = Instance.new("Part")
template.Name = "Template"
local live = Instance.new("Part")
live.Parent = workspace
live.Touched:Connect(function() end)
local n = 0
game:GetService("RunService").Heartbeat:Connect(function()
    n += 1
    cache[n % 20] = { n }
    template.Name = "Template"
    if n % 10 == 0 then
        local part = Instance.new("Part")
        part.Parent = workspace
        part:Destroy()
        local dropped = Instance.new("Part")
        dropped.Name = "Dropped"
    end
end)
"#;

const LEAKY_NAME: &str = "ServerScriptService.Leaky";
const STEADY_NAME: &str = "ServerScriptService.Steady";

fn place() -> WeakDom {
    let mut dom = conformance::bare_place();
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ServerScriptService")
        })
        .expect("ServerScriptService");
    for (name, source) in [("Leaky", LEAKY), ("Steady", STEADY)] {
        let script = dom.new_instance("Script", name, Some(sss));
        let _ = dom.set_property(script, "Source", Variant::String(source.into()));
    }
    dom.take_changes();
    dom
}

/// Steps `seconds` of game time with the dock open on the server.
fn step(session: &mut Session, seconds: f64) {
    let until = session.server.time() + seconds;
    while session.server.time() < until {
        session.sim(Peer::Server).unwrap().keep_leak_tracking();
        session.step();
    }
}

fn history(session: &Session) -> std::collections::VecDeque<Sample> {
    let tracker = session.sim(Peer::Server).unwrap().leak_tracker().unwrap();
    let history = tracker.borrow().history().clone();
    history
}

fn started() -> Session {
    let mut session = Session::new(place()).expect("the place runs");
    session.start();
    session
}

#[test]
fn scripted_leaks_are_suspects_and_the_steady_script_is_not() {
    let mut session = started();
    step(&mut session, 12.0);
    let history = history(&session);
    assert!(history.len() >= leaks::GROWING, "{} samples", history.len());
    let suspects = leaks::suspects(&history);
    let from = |script: &str| {
        suspects
            .iter()
            .filter(|s| s.site.as_ref().is_some_and(|site| site.script == script))
            .collect::<Vec<_>>()
    };
    let leaky = from(LEAKY_NAME);
    // The Touched connections on nil-parented parts, and their count.
    assert!(
        leaky
            .iter()
            .any(|s| s.kind == Kind::Signals && s.what == "Touched on a nil-parented Part"),
        "{suspects:#?}"
    );
    assert!(
        leaky.iter().any(|s| s.what == "connections keep growing"),
        "{suspects:#?}"
    );
    // The parts piling up, pinned on their Instance.new line.
    let parts = leaky
        .iter()
        .find(|s| s.kind == Kind::Instances)
        .unwrap_or_else(|| panic!("{suspects:#?}"));
    assert_eq!(parts.what, "Part parented to nil");
    assert_eq!(parts.site.as_ref().unwrap().line, 7);
    // The table holding them.
    assert!(
        leaky
            .iter()
            .any(|s| s.kind == Kind::Tables && s.what.contains("holder")),
        "{suspects:#?}"
    );
    assert!(from(STEADY_NAME).is_empty(), "{suspects:#?}");
}

#[test]
fn the_heap_is_split_per_script_and_the_template_stays_one() {
    let mut session = started();
    step(&mut session, 6.0);
    let history = history(&session);
    let last = history.back().unwrap();
    assert!(last.heap > 0);
    for name in [LEAKY_NAME, STEADY_NAME] {
        let stat = last.scripts.iter().find(|s| s.name == name);
        assert!(
            stat.is_some_and(|s| s.heap > 0),
            "{name}: {:#?}",
            last.scripts
        );
    }
    // The template and `live` were made before tracking started, so they
    // have no site: one template held out of the place, never more.
    let held: usize = last
        .instances
        .iter()
        .filter(|g| g.site.is_none() && !g.destroyed && g.class == "Part")
        .map(|g| g.held)
        .sum();
    assert_eq!(held, 1, "{:#?}", last.instances);
}

#[test]
fn a_snapshot_diff_shows_what_grew() {
    let mut session = started();
    step(&mut session, 1.0);
    let before = session.sim(Peer::Server).unwrap().leak_sample();
    step(&mut session, 3.0);
    let after = session.sim(Peer::Server).unwrap().leak_sample();
    let rows = leaks::diff(&before, &after);
    let holder = rows
        .iter()
        .find(|r| r.category == "Luau tables" && r.what.contains("holder"))
        .unwrap_or_else(|| panic!("{rows:#?}"));
    assert!(holder.delta() >= 15, "{holder:?}");
    assert!(
        rows.iter().any(|r| r.category == "Signals"
            && r.site.as_ref().is_some_and(|s| s.script == LEAKY_NAME)
            && r.delta() > 0),
        "{rows:#?}"
    );
}

#[test]
fn nothing_is_tracked_until_the_dock_asks() {
    let mut session = started();
    for _ in 0..10 {
        session.step();
    }
    assert!(session.sim(Peer::Server).unwrap().leak_tracker().is_none());
    session.sim(Peer::Server).unwrap().keep_leak_tracking();
    session.step();
    let tracker = session.sim(Peer::Server).unwrap().leak_tracker().unwrap();
    assert!(tracker.borrow().on());
    assert_eq!(tracker.borrow().history().len(), 1);
}

/// Roblox's `Destroy` disconnects every connection to the instance and
/// its descendants; parenting to nil leaves them connected.
#[test]
fn destroy_disconnects_and_a_nil_parent_does_not() {
    const SOURCE: &str = r#"
local a = Instance.new("Folder")
local child = Instance.new("Folder", a)
local ca = a.ChildRemoved:Connect(function() end)
local cc = child.Changed:Connect(function() end)
local kept = Instance.new("Folder", workspace)
local ck = kept.Changed:Connect(function() end)
kept.Parent = nil
a:Destroy()
print(ca.Connected, cc.Connected, ck.Connected)
"#;
    let mut dom = conformance::bare_place();
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ServerScriptService")
        })
        .unwrap();
    let script = dom.new_instance("Script", "Destroyer", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(SOURCE.into()));
    dom.take_changes();
    let mut session = Session::new(dom).expect("the place runs");
    session.start();
    session.run_for(0.2);
    let lines: Vec<String> = session
        .server
        .take_output()
        .lines()
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert_eq!(lines, ["false false true"]);
}

#[test]
fn growing_needs_a_rise_at_every_step() {
    assert!(leaks::growing(&[1, 2, 3, 4]));
    assert!(leaks::growing(&[9, 1, 2, 3, 4]));
    assert!(!leaks::growing(&[1, 2, 2, 4]));
    assert!(!leaks::growing(&[1, 5, 1, 5]));
    assert!(!leaks::growing(&[1, 2, 3]));
}

/// What tracking costs on a busier place: run with `--ignored
/// --nocapture`; prints the mean step with the dock shut and open, and one
/// sample's time.
#[test]
#[ignore]
fn overhead() {
    const BUSY: &str = r#"
local data = {}
for i = 1, 20000 do data[i] = { id = i, name = "item" .. i } end
local parts = {}
for i = 1, 500 do
    local p = Instance.new("Part")
    p.Anchored = true
    p.Position = Vector3.new(i % 25 * 4, 1, i // 25 * 4)
    p.Parent = workspace
    p.Touched:Connect(function() end)
    parts[i] = p
end
local beat = 0
game:GetService("RunService").Heartbeat:Connect(function()
    beat += 1
    local t = {}
    for i = 1, 200 do t[i] = i * beat end
    data[beat % 20000 + 1].hits = beat
    parts[beat % 500 + 1].Name = "p" .. beat
    if beat % 10 == 0 then Instance.new("Part"):Destroy() end
end)
"#;
    let mut dom = conformance::bare_place();
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ServerScriptService")
        })
        .unwrap();
    let script = dom.new_instance("Script", "Busy", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(BUSY.into()));
    dom.take_changes();
    let mut session = Session::new(dom).expect("the place runs");
    session.start();
    session.run_for(1.0);
    let frames = 600;
    let time = |session: &mut Session, track: bool| {
        let started = std::time::Instant::now();
        for _ in 0..frames {
            if track {
                session.sim(Peer::Server).unwrap().keep_leak_tracking();
            }
            session.step();
        }
        started.elapsed().as_secs_f64() * 1000.0 / frames as f64
    };
    let shut = time(&mut session, false);
    let open = time(&mut session, true);
    // Tracking stops a second (of wall time) after the dock last asked.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let shut_again = time(&mut session, false);
    let sim = session.sim(Peer::Server).unwrap();
    let started = std::time::Instant::now();
    let sample = sim.leak_sample();
    let one = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "step shut {shut:.3} ms, open {open:.3} ms (a sample every {} s), shut again \
         {shut_again:.3} ms; one sample {one:.2} ms, {} tables kept, truncated {}",
        leaks::SAMPLE_EVERY,
        sample.tables.len(),
        sample.truncated
    );
}
