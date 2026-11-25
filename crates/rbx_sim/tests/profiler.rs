//! What the MicroProfiler dock reads off a run: each frame's costs and
//! `debug.profilebegin` scopes, and the ScriptProfiler's export.

use rbx_dom::{Variant, WeakDom};
use rbx_sim::{conformance, Peer, Session};

const SOURCE: &str = r#"
    local function work()
        local x = 0
        for i = 1, 20000 do
            x += math.sqrt(i)
        end
        return x
    end
    game:GetService("RunService").Heartbeat:Connect(function()
        debug.profilebegin("outer")
        debug.profilebegin("inner")
        work()
        debug.profileend()
        debug.profileend()
    end)
"#;

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
    let script = dom.new_instance("Script", "Busy", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(SOURCE.into()));
    dom.take_changes();
    dom
}

#[test]
fn frames_carry_their_costs_and_scopes() {
    let mut session = Session::new(place()).expect("the place runs");
    session.start();
    for _ in 0..10 {
        session.step();
    }
    let frames = session.frames();
    assert_eq!(frames.len(), 10);
    let last = frames.back().unwrap();
    assert!(last.script > std::time::Duration::ZERO, "{last:?}");
    let names: Vec<(&str, u16)> = last
        .labels
        .iter()
        .map(|(peer, l)| {
            assert_eq!(*peer, Peer::Server);
            (l.name.as_str(), l.depth)
        })
        .collect();
    assert_eq!(names, vec![("inner", 1), ("outer", 0)]);
    let (inner, outer) = (&last.labels[0].1, &last.labels[1].1);
    assert!(outer.start <= inner.start && inner.end <= outer.end);
    session.set_render_time(std::time::Duration::from_millis(3));
    assert_eq!(
        session.frames().back().unwrap().render,
        std::time::Duration::from_millis(3)
    );
}

#[test]
fn the_script_profiler_exports_heartbeat_work() {
    let mut session = Session::new(place()).expect("the place runs");
    session.start();
    let server = session.sim(Peer::Server).unwrap().profiler();
    server.borrow_mut().start(10_000);
    for _ in 0..30 {
        session.step();
    }
    server.borrow_mut().stop();
    let json: serde_json::Value = serde_json::from_str(&server.borrow().data()).unwrap();
    assert_eq!(json["Version"], 2);
    let categories: Vec<&str> = json["Categories"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["Name"].as_str())
        .collect();
    assert!(categories.contains(&"Heartbeat"), "{json}");
    let work = json["Functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["Name"] == "work")
        .unwrap_or_else(|| panic!("no work() in {json}"));
    assert_eq!(work["Source"], "ServerScriptService.Busy");
    assert_eq!(work["Line"], 2);
    assert!(work["TotalDuration"].as_u64().unwrap() > 0);
}
