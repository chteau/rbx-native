//! Local Run's simulated network (`rbx_sim::link`): latency, jitter and
//! loss between the server and its clients, and the documented limits of
//! remotes across it.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::link::{Network, Way};
use rbx_sim::{conformance, Peer, Session};

fn service(dom: &WeakDom, class: &str) -> Ref {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
        .unwrap_or_else(|| panic!("no {class}"))
}

/// A bare place with a server Script, a LocalScript in
/// StarterPlayerScripts, and a `Ping` RemoteEvent and `Fast`
/// UnreliableRemoteEvent in ReplicatedStorage.
fn place(server: &str, client: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let sss = service(&dom, "ServerScriptService");
    let script = dom.new_instance("Script", "Server", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(server.into()));
    let starter_player = service(&dom, "StarterPlayer");
    let starter = dom.get(starter_player).unwrap().children()[0];
    let local = dom.new_instance("LocalScript", "Client", Some(starter));
    let _ = dom.set_property(local, "Source", Variant::String(client.into()));
    let rs = service(&dom, "ReplicatedStorage");
    dom.new_instance("RemoteEvent", "Ping", Some(rs));
    dom.new_instance("UnreliableRemoteEvent", "Fast", Some(rs));
    dom.take_changes();
    dom
}

fn run(dom: WeakDom, network: Network, frames: usize) -> Vec<(Peer, String)> {
    let mut session = Session::new(dom).expect("the place runs");
    session.set_network(network);
    session.start();
    session.join("Player1", -1, None).expect("joins");
    let mut lines = Vec::new();
    for _ in 0..frames {
        session.step();
        for (peer, output) in session.take_output() {
            lines.extend(output.lines().iter().map(|l| (peer, l.clone())));
        }
    }
    lines
}

fn way(latency: f64, jitter: f64, loss: f64) -> Way {
    Way {
        latency,
        jitter,
        loss,
    }
}

fn even(latency: f64) -> Network {
    Network {
        down: way(latency, 0.0, 0.0),
        up: way(latency, 0.0, 0.0),
    }
}

/// The number a line `<prefix> <number>` printed by `peer` carries.
fn number(lines: &[(Peer, String)], peer: Peer, prefix: &str) -> f64 {
    lines
        .iter()
        .find_map(|(p, l)| {
            (*p == peer)
                .then(|| l.strip_prefix(prefix))
                .flatten()
                .and_then(|rest| rest.trim().parse().ok())
        })
        .unwrap_or_else(|| panic!("no `{prefix}` from {peer:?}: {lines:#?}"))
}

const FRAME: f64 = 1.0 / 60.0;

const LATE_FOLDER: (&str, &str) = (
    "task.wait(1) local f = Instance.new('Folder') f.Name = 'Late' f.Parent = workspace \
     print('made', time()) \
     game.ReplicatedStorage.Ping.OnServerEvent:Connect(function(player) game.ReplicatedStorage.Ping:FireClient(player) end)",
    "local stats = game:GetService('Stats') \
     print('ping', game:GetService('Players').LocalPlayer:GetNetworkPing()) \
     print('data ping', stats.Network.ServerStatsItem['Data Ping']:GetValue()) \
     workspace:WaitForChild('Late') print('seen', time()) \
     local sent = time() game.ReplicatedStorage.Ping.OnClientEvent:Connect(function() print('round trip', time() - sent) end) \
     game.ReplicatedStorage.Ping:FireServer() \
     task.wait(1) print('receiving', stats.DataReceiveKbps > 0)",
);

#[test]
fn no_added_latency_delivers_on_the_same_frame() {
    let lines = run(place(LATE_FOLDER.0, LATE_FOLDER.1), Network::default(), 180);
    let made = number(&lines, Peer::Server, "made");
    let seen = number(&lines, Peer::Client(0), "seen");
    // Same session step: a client's clock reads one frame behind the server's.
    assert!((made - seen - FRAME).abs() < 1e-9, "{lines:#?}");
    assert_eq!(number(&lines, Peer::Client(0), "ping"), 0.0);
}

#[test]
fn latency_delays_replication_and_remotes_and_shows_in_the_ping() {
    let lines = run(place(LATE_FOLDER.0, LATE_FOLDER.1), even(0.1), 240);
    let made = number(&lines, Peer::Server, "made");
    let seen = number(&lines, Peer::Client(0), "seen");
    assert!(
        (seen + FRAME - made - 0.1).abs() < 1e-9,
        "made {made}, seen {seen}"
    );
    let round_trip = number(&lines, Peer::Client(0), "round trip");
    assert!(
        (0.2..0.25).contains(&round_trip),
        "round trip {round_trip}: {lines:#?}"
    );
    assert!((number(&lines, Peer::Client(0), "ping") - 0.2).abs() < 1e-9);
    assert!((number(&lines, Peer::Client(0), "data ping") - 200.0).abs() < 1e-6);
    assert!(
        lines.contains(&(Peer::Client(0), "receiving true".into())),
        "{lines:#?}"
    );
}

#[test]
fn remote_events_stay_in_order_through_loss_and_jitter_and_unreliable_ones_do_not() {
    let lines = run(
        place(
            "local ping, fast = game.ReplicatedStorage.Ping, game.ReplicatedStorage.Fast \
             game:GetService('Players').PlayerAdded:Wait() task.wait(1) \
             local player = game:GetService('Players'):GetPlayers()[1] \
             for i = 1, 200 do ping:FireClient(player, i) fast:FireClient(player, i) if i % 4 == 0 then task.wait() end end",
            "local got, fast, inOrder = {}, {}, true \
             game.ReplicatedStorage.Ping.OnClientEvent:Connect(function(i) table.insert(got, i) end) \
             game.ReplicatedStorage.Fast.OnClientEvent:Connect(function(i) \
               if #fast > 0 and i < fast[#fast] then inOrder = false end table.insert(fast, i) end) \
             task.wait(4) \
             local ordered = #got == 200 for i, v in got do if v ~= i then ordered = false end end \
             print('reliable', ordered) print('unreliable', #fast < 200, #fast > 100, inOrder)",
        ),
        Network {
            down: way(0.05, 0.03, 0.2),
            up: way(0.05, 0.03, 0.2),
        },
        360,
    );
    assert!(
        lines.contains(&(Peer::Client(0), "reliable true".into())),
        "{lines:#?}"
    );
    assert!(
        lines.contains(&(Peer::Client(0), "unreliable true true false".into())),
        "{lines:#?}"
    );
}

#[test]
fn an_unreliable_payload_over_1000_bytes_is_dropped_with_a_warning() {
    let lines = run(
        place(
            "game.ReplicatedStorage.Fast.OnServerEvent:Connect(function(_, s) print('got', #s) end)",
            "local fast = game.ReplicatedStorage.Fast \
             fast:FireServer(string.rep('x', 2000)) fast:FireServer(string.rep('x', 900))",
        ),
        Network::default(),
        60,
    );
    assert!(
        lines.contains(&(Peer::Server, "got 900".into())),
        "{lines:#?}"
    );
    assert!(
        !lines.contains(&(Peer::Server, "got 2000".into())),
        "{lines:#?}"
    );
    assert!(
        lines.iter().any(|(p, l)| *p == Peer::Client(0)
            && l.contains("ReplicatedStorage.Fast")
            && l.contains("1003 bytes over the 1000 byte limit")),
        "{lines:#?}"
    );
}

#[test]
fn an_unreliable_event_with_no_listener_is_discarded_not_queued() {
    let lines = run(
        place(
            "local fast, ping = game.ReplicatedStorage.Fast, game.ReplicatedStorage.Ping \
             task.wait(1) fast.OnServerEvent:Connect(function(_, n) print('fast', n) end) \
             ping.OnServerEvent:Connect(function(_, n) print('ping', n) end)",
            "game.ReplicatedStorage.Fast:FireServer(1) game.ReplicatedStorage.Ping:FireServer(1) \
             task.wait(2) game.ReplicatedStorage.Fast:FireServer(2)",
        ),
        Network::default(),
        240,
    );
    assert!(
        lines.contains(&(Peer::Server, "ping 1".into())),
        "{lines:#?}"
    );
    assert!(
        lines.contains(&(Peer::Server, "fast 2".into())),
        "{lines:#?}"
    );
    assert!(
        !lines.contains(&(Peer::Server, "fast 1".into())),
        "{lines:#?}"
    );
    assert!(
        lines.iter().any(|(p, l)| *p == Peer::Server
            && l.starts_with("Remote event invocation discarded for ReplicatedStorage.Fast")),
        "{lines:#?}"
    );
}

#[test]
fn the_server_takes_about_500_unreliable_events_a_second_from_a_client() {
    let lines = run(
        place(
            "local n = 0 game.ReplicatedStorage.Fast.OnServerEvent:Connect(function() n += 1 end) \
             task.wait(4) print('took', n)",
            "local fast = game.ReplicatedStorage.Fast \
             for _ = 1, 30 do for _ = 1, 50 do fast:FireServer() end task.wait() end",
        ),
        Network::default(),
        300,
    );
    let took = number(&lines, Peer::Server, "took");
    assert!((500.0..=1000.0).contains(&took), "took {took}");
}

#[test]
fn a_client_owned_character_reaches_the_server_late() {
    // The client walks its character; behind 100 ms each way the server's
    // copy of the root trails the client's by the uplink.
    let mut session = Session::new(place("", "")).unwrap();
    session.set_network(even(0.1));
    session.start();
    session.join("Player1", -1, None).unwrap();
    session.run_for(1.5);
    let root_z = |s: &Session, peer| {
        let sim = s.sim(peer).unwrap();
        let (player, _) = s.clients[0].sim.local_player().unwrap();
        let dom = sim.dom();
        let Some(Variant::Ref(character)) = dom.get(player).unwrap().properties().get("Character")
        else {
            panic!("no character")
        };
        let root = dom
            .get(*character)
            .unwrap()
            .children()
            .iter()
            .copied()
            .find(|c| dom.get(*c).is_some_and(|i| i.name() == "HumanoidRootPart"))
            .unwrap();
        match dom.get(root).unwrap().properties().get("CFrame") {
            Some(Variant::CFrame(cf)) => cf.position.z,
            _ => panic!("no CFrame"),
        }
    };
    session.input(
        0,
        rbx_sim::Input::Key {
            code: "W".into(),
            down: true,
        },
    );
    session.run_for(1.0);
    let client = root_z(&session, Peer::Client(0));
    let server = root_z(&session, Peer::Server);
    // Walking at 16 studs/s, 0.1 s behind is 1.6 studs.
    assert!(
        (1.0..2.5).contains(&(server - client)),
        "client z {client}, server z {server}"
    );
}

#[test]
fn remote_order_holds_across_a_link_like_the_recording_s() {
    // The RemoteOrder golden was recorded across a ~49 ms round trip; the
    // same link here keeps every ordering field, and the timings in beats
    // land with it: the server's InvokeClient takes 3 server beats, as in
    // every recorded pass, and an event round trip falls in the recorded
    // range.
    let only = vec!["RemoteOrder".to_owned()];
    let ours = conformance::run_on_network(600.0, &only, even(0.0245)).expect("runs");
    let ours = &ours["RemoteOrder"];
    let golden = conformance::golden("RemoteOrder").expect("golden");
    for field in [
        "c2s_order",
        "s2c_order",
        "s2c_values",
        "invoke_client",
        "invoke_server",
        "s2c_arrival_beats",
        "_invoke_client_beats",
    ] {
        assert_eq!(ours[field], golden["result"][field], "{field}: {ours:#?}");
    }
    let recorded: Vec<f64> = golden["raw"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|pass| pass["_event_rtt_beats"].as_f64())
        .collect();
    let (low, high) = recorded
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), b| (l.min(*b), h.max(*b)));
    let rtt = ours["_event_rtt_beats"].as_f64().unwrap();
    assert!(
        (low..=high).contains(&rtt),
        "{rtt} beats, recorded {recorded:?}"
    );
}
