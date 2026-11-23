use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};

use super::*;
use crate::bots::Behaviour;
use crate::replay::{Action, Logged};
use crate::Peer;

fn child(dom: &mut WeakDom, class: &str, name: &str, parent: Ref) -> Ref {
    dom.new_instance(class, name, Some(parent))
}

fn service(dom: &WeakDom, class: &str) -> Ref {
    *dom.root_refs()
        .iter()
        .find(|r| dom.get(**r).is_some_and(|i| i.class() == class))
        .expect("bare_place has it")
}

fn block(dom: &mut WeakDom, at: [f32; 3], size: [f32; 3]) {
    let workspace = service(dom, "Workspace");
    let part = child(dom, "Part", "Step", workspace);
    let v = |[x, y, z]: [f32; 3]| Vector3Data { x, y, z };
    let _ = dom.set_property(part, "Anchored", Variant::Bool(true));
    let _ = dom.set_property(part, "Size", Variant::Vector3(v(size)));
    let _ = dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: v(at),
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
}

fn script(dom: &mut WeakDom, class: &str, parent: Ref, source: &str) {
    let made = child(dom, class, class, parent);
    let _ = dom.set_property(made, "Source", Variant::String(source.into()));
}

/// A baseplate with a spawn and a few low steps around it.
fn obby() -> WeakDom {
    let mut dom = crate::conformance::bare_place();
    for i in 0..6 {
        let angle = i as f32;
        block(
            &mut dom,
            [angle.cos() * 30.0, 1.0, angle.sin() * 30.0],
            [6.0, 2.0, 6.0],
        );
    }
    dom.take_changes();
    dom
}

fn config(bots: usize, mix: Mix) -> Config {
    Config {
        bots,
        ramp: None,
        duration: None,
        seed: 42,
        mix,
        min_hz: DEFAULT_MIN_HZ,
    }
}

fn only(behaviour: Behaviour) -> Mix {
    let mut mix = Mix {
        idle: 0,
        wander: 0,
        follow: 0,
        click: 0,
        remotes: 0,
    };
    match behaviour {
        Behaviour::Idle => mix.idle = 1,
        Behaviour::Wander => mix.wander = 1,
        Behaviour::Follow => mix.follow = 1,
        Behaviour::Click => mix.click = 1,
        Behaviour::Remotes => mix.remotes = 1,
    }
    mix
}

fn frames(test: &mut LoadTest, n: u64) {
    for _ in 0..n {
        test.step().expect("steps");
    }
}

/// Each bot's character root on the server, by bot name.
fn positions(test: &LoadTest) -> Vec<(String, [i64; 3])> {
    let dom = test.session.server.dom();
    test.session
        .clients
        .iter()
        .filter_map(|c| {
            let character = crate::humanoid::reference(&dom, c.player, "Character")?;
            let root = dom
                .get(character)?
                .children()
                .iter()
                .copied()
                .find(|r| dom.get(*r).is_some_and(|i| i.name() == "HumanoidRootPart"))?;
            let Some(Variant::CFrame(cf)) = dom.get(root)?.properties().get("CFrame") else {
                return None;
            };
            let p = cf.position;
            let round = |v: f32| (v * 10.0).round() as i64;
            Some((c.name.clone(), [round(p.x), round(p.y), round(p.z)]))
        })
        .collect()
}

#[test]
fn bots_join_and_leave_as_real_clients() {
    let mut test = LoadTest::new(obby(), "obby", config(3, only(Behaviour::Idle)), None).unwrap();
    frames(&mut test, 1);
    let names: Vec<_> = test
        .session
        .clients
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(names, ["Bot1", "Bot2", "Bot3"]);
    let dom = test.session.server.dom();
    let ids: Vec<_> = test
        .session
        .clients
        .iter()
        .map(|c| {
            dom.get(c.player)
                .unwrap()
                .properties()
                .get("UserId")
                .cloned()
        })
        .collect();
    drop(dom);
    assert_eq!(
        ids,
        [-1, -2, -3].map(|n| Some(Variant::Int64(n))),
        "negative UserIds, as test players"
    );
    test.set_target(1);
    frames(&mut test, 1);
    let names: Vec<_> = test
        .session
        .clients
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["Bot1"]);
    let players = test
        .session
        .server
        .dom()
        .get(service(&test.session.server.dom(), "Players"))
        .unwrap()
        .children()
        .len();
    assert_eq!(players, 1, "the server's Players went with them");
}

#[test]
fn wandering_bots_walk_somewhere() {
    let mut test = LoadTest::new(obby(), "obby", config(2, only(Behaviour::Wander)), None).unwrap();
    frames(&mut test, 90);
    let before = positions(&test);
    frames(&mut test, 360);
    let after = positions(&test);
    assert_eq!(before.len(), 2);
    for ((name, a), (_, b)) in before.iter().zip(&after) {
        let moved = (((a[0] - b[0]).pow(2) + (a[2] - b[2]).pow(2)) as f64).sqrt() / 10.0;
        assert!(moved > 4.0, "{name} moved {moved} studs: {a:?} -> {b:?}");
    }
}

/// The bots decide from the seed alone; where a player spawns on a
/// SpawnLocation is the place's own randomness, so this one's spawn is 6x6,
/// which puts everyone at its centre.
#[test]
fn the_same_seed_plays_the_same_run() {
    let mut place = obby();
    let spawn = service(&place, "Workspace");
    let spawn = place
        .get(spawn)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|r| place.get(*r).unwrap().class() == "SpawnLocation")
        .unwrap();
    let _ = place.set_property(
        spawn,
        "Size",
        Variant::Vector3(Vector3Data {
            x: 6.0,
            y: 1.0,
            z: 6.0,
        }),
    );
    place.take_changes();
    let run = |seed: u64| {
        let mut cfg = config(4, Mix::default());
        cfg.seed = seed;
        let mut test = LoadTest::new(place.clone(), "obby", cfg, None).unwrap();
        frames(&mut test, 300);
        (test.bots.roster(), positions(&test))
    };
    let a = run(9);
    assert_eq!(a, run(9));
    assert_ne!(a, run(10));
}

#[test]
fn the_ramp_adds_bots_on_schedule() {
    let mut cfg = config(5, only(Behaviour::Idle));
    cfg.ramp = Some(Ramp::parse("2/1s").unwrap());
    cfg.duration = Some(4.0);
    let mut test = LoadTest::new(obby(), "obby", cfg, None).unwrap();
    while !test.done() {
        test.step().unwrap();
    }
    let counts: Vec<_> = test.samples().iter().map(|s| s.bots).collect();
    assert_eq!(counts, [2, 4, 5, 5]);
    // A drop in the target is at once.
    test.set_target(1);
    frames(&mut test, 1);
    assert_eq!(test.bots.len(), 1);
}

#[test]
fn ramps_and_durations_parse() {
    assert_eq!(
        Ramp::parse("5/10s"),
        Ok(Ramp {
            step: 5,
            every: 10.0
        })
    );
    assert_eq!(Ramp::parse("3/2m").map(|r| r.every), Ok(120.0));
    assert!(Ramp::parse("0/10s").is_err() && Ramp::parse("5").is_err());
    assert_eq!(parse_seconds("120s"), Ok(120.0));
    assert_eq!(parse_seconds("2m"), Ok(120.0));
    assert_eq!(parse_seconds("90"), Ok(90.0));
    assert!(parse_seconds("soon").is_err());
    assert!(config(MAX_BOTS + 1, Mix::default()).check().is_err());
    assert!(config(DEFAULT_CAP + 1, Mix::default())
        .check()
        .unwrap()
        .is_some());
    assert_eq!(config(DEFAULT_CAP, Mix::default()).check(), Ok(None));
}

/// A place whose server reads a DataStore and writes a MemoryStore once a
/// second per player, and whose clients fire a remote once a second.
fn busy() -> WeakDom {
    let mut dom = obby();
    let storage = service(&dom, "ReplicatedStorage");
    child(&mut dom, "RemoteEvent", "Ping", storage);
    let server = service(&dom, "ServerScriptService");
    script(
        &mut dom,
        "Script",
        server,
        r#"
local ds = game:GetService("DataStoreService"):GetDataStore("loadtest")
local map = game:GetService("MemoryStoreService"):GetSortedMap("loadtest")
game.ReplicatedStorage.Ping.OnServerEvent:Connect(function(player, n)
    ds:GetAsync("k" .. player.UserId)
    map:SetAsync("k", n, 60)
end)
"#,
    );
    let starter = service(&dom, "StarterPlayer");
    let scripts = dom
        .get(starter)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|r| dom.get(*r).unwrap().class() == "StarterPlayerScripts")
        .unwrap();
    script(
        &mut dom,
        "LocalScript",
        scripts,
        r#"
while true do
    game.ReplicatedStorage.Ping:FireServer(1.5)
    task.wait(1)
end
"#,
    );
    dom.take_changes();
    dom
}

#[test]
fn the_report_has_the_servers_numbers() {
    let mut cfg = config(3, only(Behaviour::Idle));
    cfg.duration = Some(4.0);
    let mut test = LoadTest::new(busy(), "busy", cfg, None).unwrap();
    while !test.done() {
        test.step().unwrap();
    }
    let report = test.finish();
    let last = report.samples.last().unwrap();
    assert_eq!(report.samples.len(), 4);
    assert_eq!(last.bots, 3);
    assert!(last.server_ms > 0.0 && last.heartbeat_hz > 0.0 && last.heartbeat_hz <= 60.0);
    assert!(last.heap_mb > 0.0 && last.client_heap_mb > 0.0);
    assert!(last.instances > 50 && last.parts >= 8, "{last:?}");
    // Three clients a second, each answered with a GetAsync and a SetAsync.
    assert!((2.0..=4.0).contains(&last.remotes_in), "{last:?}");
    assert!(last.kb_in > 0.0);
    // The calls in the run's first four seconds.
    assert!(
        last.datastore_by_kind
            .get("GetAsync")
            .is_some_and(|r| *r >= 6.0),
        "{last:?}"
    );
    assert!(last.memorystore_per_min >= 6.0, "{last:?}");
    assert!(report
        .budgets
        .iter()
        .any(|l| l.kind == "GetAsync" && l.budget == 90.0));
    assert!(report.per_bot.client_ms > 0.0);
    let md = report.markdown();
    assert!(
        md.starts_with("# Load test: busy") && md.contains("| GetAsync |"),
        "{md}"
    );
    let json = report.json();
    assert_eq!(json["samples"].as_array().unwrap().len(), 4);
    assert_eq!(json["passed"], serde_json::json!(report.passed()));
}

#[test]
fn a_server_under_the_rate_for_long_enough_breaks() {
    let mut cfg = config(2, only(Behaviour::Idle));
    // Nothing keeps 61 Hz: it breaks at the bots of its first second.
    cfg.min_hz = 61.0;
    cfg.duration = Some(4.0);
    let mut test = LoadTest::new(obby(), "obby", cfg, None).unwrap();
    while !test.done() {
        test.step().unwrap();
    }
    assert_eq!(test.breaking_point(), Some(2));
    assert!(!test.report().passed());
}

#[test]
fn remote_bots_fire_what_clients_sent() {
    let fired = |behaviour| {
        let mut test = LoadTest::new(busy(), "busy", config(2, only(behaviour)), None).unwrap();
        frames(&mut test, 600);
        let total: f64 = test.samples().iter().map(|s| s.remotes_in).sum();
        let ping = test.session.server.dom().root_refs().to_vec();
        let ping = ping
            .into_iter()
            .find_map(|r| {
                let dom = test.session.server.dom();
                let storage = dom.get(r).filter(|i| i.class() == "ReplicatedStorage")?;
                storage.children().first().copied()
            })
            .unwrap();
        (total, test.session.remote_args(ping).to_vec())
    };
    let (idle, _) = fired(Behaviour::Idle);
    let (busy, seen) = fired(Behaviour::Remotes);
    // On top of the LocalScripts' one a second each, a shot every 0.5-3 s.
    assert!(busy > idle + 5.0, "{busy} vs {idle}");
    // What the bots sent was what clients sent, nudged.
    assert!(seen.iter().all(|args| matches!(args.as_slice(), [rbx_lua::server::Wire::Number(n)] if (1.3..1.7).contains(n))), "{seen:?}");
}

#[test]
fn followers_play_a_recording_back() {
    let recording = Recording {
        actions: [(0, true), (100, false)]
            .into_iter()
            .map(|(frame, down)| Logged {
                frame,
                peer: Peer::Client(0),
                action: Action::Input(crate::Input::Key {
                    code: "W".into(),
                    down,
                }),
            })
            .collect(),
        ..Recording::default()
    };
    let mut test = LoadTest::new(
        obby(),
        "obby",
        config(1, only(Behaviour::Follow)),
        Some(&recording),
    )
    .unwrap();
    frames(&mut test, 60);
    let before = positions(&test);
    frames(&mut test, 120);
    let after = positions(&test);
    assert_eq!(test.bots.roster()[0].1, Behaviour::Follow);
    assert_ne!(before, after, "walked forward while W was held");
}

#[test]
fn a_fire_action_survives_a_recording() {
    use rbx_lua::server::Wire;
    let recording = Recording {
        actions: vec![Logged {
            frame: 3,
            peer: Peer::Client(1),
            action: Action::Fire {
                remote: Ref::new(77),
                args: vec![
                    Wire::Number(1.5),
                    Wire::String(b"hi".to_vec()),
                    Wire::Data(
                        Variant::Vector3(Vector3Data {
                            x: 1.0,
                            y: 2.0,
                            z: 3.0,
                        }),
                        "Vector3".into(),
                    ),
                    Wire::Table(vec![(Wire::Number(1.0), Wire::Bool(true))]),
                ],
            },
        }],
        ..Recording::default()
    };
    let back = Recording::from_bytes(&recording.to_bytes()).unwrap();
    assert_eq!(back.actions, recording.actions);
}
