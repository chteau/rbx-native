//! Record and replay: a smoke fixture run by the bot, recorded, replays
//! with no divergence; an input changed in the recording is caught on the
//! frame it lands; what scripts read from outside (clocks, randomness,
//! GUIDs) comes back the same.

#[allow(dead_code)]
#[path = "../examples/support/smoke.rs"]
mod smoke;

use rbx_dom::Variant;
use rbx_sim::replay::{Action, Recording, Replay};
use rbx_sim::{conformance, Input, Peer, Session};

fn fixture(name: &str) -> rbx_dom::WeakDom {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/smoke")
        .join(name);
    rbx_sim::load::read_place(&path).unwrap()
}

fn replay_all(recording: Recording) -> Replay {
    let mut replay = Replay::new(recording).unwrap();
    if let Err(divergence) = replay.run_to(u64::MAX) {
        panic!("{divergence}");
    }
    replay
}

#[test]
fn recorded_smoke_runs_replay_without_divergence() {
    for name in [
        "combat.rbxlx",
        "obby.rbxlx",
        "shop.rbxlx",
        "streaming.rbxlx",
    ] {
        let (report, recording) = smoke::record(fixture(name), 8.0, 7).unwrap();
        assert!(
            report.errors().next().is_none(),
            "{name}: {}",
            report.transcript()
        );
        assert_eq!(recording.frame_count(), 480, "{name}");
        // Through the file format, as a saved recording would be.
        let recording = Recording::from_bytes(&recording.to_bytes()).unwrap();
        let replay = replay_all(recording);
        assert!(replay.errors().is_empty(), "{name}: {:?}", replay.errors());
    }
}

#[test]
fn a_changed_input_is_caught_on_the_frame_it_lands() {
    let (_, mut recording) = smoke::record(fixture("combat.rbxlx"), 8.0, 7).unwrap();
    let walk = recording
        .actions
        .iter_mut()
        .find(
            |l| matches!(&l.action, Action::Input(Input::Key { code, down: true }) if code == "W"),
        )
        .expect("the bot walks");
    let frame = walk.frame;
    walk.action = Action::Input(Input::Key {
        code: "S".into(),
        down: true,
    });
    let mut replay = Replay::new(recording).unwrap();
    let divergence = replay.run_to(u64::MAX).expect_err("walking back diverges");
    assert_eq!(divergence.frame, frame, "{divergence}");
    assert_eq!(divergence.peer, Peer::Client(0), "{divergence}");
}

#[test]
fn clocks_randomness_and_guids_replay_as_recorded() {
    let script = "local http = game:GetService('HttpService') \
        local r = Random.new() \
        task.wait(0.5) \
        print('drew', math.random(), math.random(1, 1000), r:NextNumber(), \
            http:GenerateGUID(false), os.time(), os.clock(), tick(), \
            DateTime.now().UnixTimestampMillis, workspace:GetServerTimeNow(), os.date('%H:%M:%S'))";
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
    let s = dom.new_instance("Script", "Draw", Some(sss));
    let _ = dom.set_property(s, "Source", Variant::String(script.into()));
    dom.take_changes();

    let mut session = Session::new(dom).unwrap();
    session.record(99);
    session.start();
    session.run_for(1.0);
    let printed = |output: Vec<(Peer, rbx_lua::Output)>| -> Vec<String> {
        output
            .into_iter()
            .flat_map(|(_, o)| o.lines().to_vec())
            .filter(|l| l.starts_with("drew"))
            .collect()
    };
    let recorded = printed(session.take_output());
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    let recording = session.recording().unwrap();
    let mut replay = Replay::new(recording).unwrap();
    replay.run_to(u64::MAX).unwrap();
    let replayed: Vec<String> = replay
        .output
        .iter()
        .flat_map(|(_, _, o)| o.lines().to_vec())
        .filter(|l| l.starts_with("drew"))
        .collect();
    assert_eq!(recorded, replayed);
}
