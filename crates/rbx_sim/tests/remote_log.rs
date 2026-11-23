//! The Remote inspector's capture (`rbx_sim::remote_log`): every remote
//! call crossing the link, its bytes, its caller and its fate.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::link::{Network, Way};
use rbx_sim::remote_log::{Direction, Event, Fate, Kind, RemoteCall, Reply};
use rbx_sim::{conformance, Session};

fn service(dom: &WeakDom, class: &str) -> Ref {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
        .unwrap_or_else(|| panic!("no {class}"))
}

/// A bare place with a server Script, a LocalScript in
/// StarterPlayerScripts and `Ping` (RemoteEvent), `Fast`
/// (UnreliableRemoteEvent) and `Ask` (RemoteFunction) in ReplicatedStorage.
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
    dom.new_instance("RemoteFunction", "Ask", Some(rs));
    dom.take_changes();
    dom
}

/// Runs `frames` frames with `players` players, capturing from the start.
fn capture(dom: WeakDom, network: Network, players: usize, frames: usize) -> Vec<Event> {
    let mut session = Session::new(dom).expect("the place runs");
    session.set_network(network);
    session.capture_remotes(true);
    session.start();
    for n in 1..=players {
        session
            .join(&format!("Player{n}"), -(n as i64), None)
            .expect("joins");
    }
    let mut events = Vec::new();
    for _ in 0..frames {
        session.step();
        events.extend(session.take_remote_events());
    }
    events
}

fn calls(events: &[Event]) -> Vec<&RemoteCall> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Call(call) => Some(call),
            _ => None,
        })
        .collect()
}

fn fates(events: &[Event], call: u64) -> Vec<Fate> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Call(c) if c.id == call => Some(c.fate),
            Event::Fate(id, fate) if *id == call => Some(*fate),
            _ => None,
        })
        .collect()
}

const PING: &str = "local rs = game.ReplicatedStorage \
    rs.Ping.OnServerEvent:Connect(function(player, ...) \
        rs.Ping:FireClient(player, 'back') \
        rs.Ping:FireAllClients(player) \
    end)";

#[test]
fn every_call_each_way_with_its_player_bytes_and_caller() {
    let client = "local rs = game.ReplicatedStorage\n\
        task.wait(0.5)\n\
        rs.Ping:FireServer(7, 'hi', Vector3.new(1, 2, 3), { part = workspace, n = 2 })\n";
    let events = capture(place(PING, client), Network::default(), 2, 120);
    let calls = calls(&events);
    let up: Vec<_> = calls
        .iter()
        .filter(|c| c.direction == Direction::ToServer)
        .collect();
    assert_eq!(up.len(), 2, "{calls:#?}");
    let first = up[0];
    assert_eq!(first.path, "ReplicatedStorage.Ping");
    assert_eq!(first.kind, Kind::Event);
    assert_eq!(first.fate, Fate::Delivered);
    // Header 8; 7 → 9; "hi" → 3 + 2; Vector3 → 1 + 12; the table 3 + its
    // entries ("part" 7 + instance 9, "n" 4 + number 9).
    assert_eq!(first.bytes, 8 + 9 + 5 + 13 + (3 + 16 + 13));
    let caller = first.caller.as_ref().expect("the caller");
    assert!(caller.script.ends_with("Client"), "{caller:?}");
    assert_eq!(caller.line, 3);
    let kinds: Vec<_> = first.args.iter().map(|a| a.kind.as_str()).collect();
    assert_eq!(kinds, ["number", "string", "Vector3", "table"]);
    assert_eq!(first.args[2].text, "1, 2, 3");
    let table = &first.args[3];
    let part = table
        .children
        .iter()
        .find(|a| a.key.as_deref() == Some("part"))
        .expect("the part entry");
    assert_eq!(
        (part.kind.as_str(), part.text.as_str()),
        ("Instance", "Workspace")
    );

    let mut players: Vec<_> = up.iter().map(|c| c.player.as_str()).collect();
    players.sort();
    assert_eq!(players, ["Player1", "Player2"]);

    // Each FireServer: a FireClient back to its sender, a FireAllClients
    // reaching both players.
    let to_one: Vec<_> = calls
        .iter()
        .filter(|c| c.direction == Direction::ToClient)
        .collect();
    assert_eq!(to_one.len(), 2);
    assert!(to_one.iter().all(|c| c.caller.as_ref().unwrap().line == 1));
    let to_all = calls
        .iter()
        .filter(|c| c.direction == Direction::ToAll)
        .count();
    assert_eq!(to_all, 4);
}

#[test]
fn nothing_is_logged_unless_capturing() {
    let client = "task.wait(0.5) game.ReplicatedStorage.Ping:FireServer(1)";
    let mut session = Session::new(place(PING, client)).expect("the place runs");
    session.start();
    session.join("Player1", -1, None).expect("joins");
    for _ in 0..90 {
        session.step();
    }
    assert!(!session.capturing_remotes());
    assert!(session.take_remote_events().is_empty());
}

#[test]
fn an_unreliable_payload_over_1000_bytes_is_flagged_and_never_leaves() {
    let client = "task.wait(0.5) \
        game.ReplicatedStorage.Fast:FireServer(string.rep('x', 996)) \
        game.ReplicatedStorage.Fast:FireServer(string.rep('x', 998))";
    let server = "game.ReplicatedStorage.Fast.OnServerEvent:Connect(function() end)";
    let events = capture(place(server, client), Network::default(), 1, 90);
    let calls = calls(&events);
    let fates: Vec<_> = calls.iter().map(|c| (c.bytes, c.fate)).collect();
    // 3 + 996 = 999 bytes of payload goes; 1001 doesn't.
    assert_eq!(
        fates,
        [(8 + 999, Fate::Delivered), (8 + 1001, Fate::OverLimit)]
    );
}

#[test]
fn unreliable_packets_past_500_a_second_are_flagged_as_rate_limited() {
    let client = "task.wait(0.5) for i = 1, 600 do game.ReplicatedStorage.Fast:FireServer(i) end";
    let server = "game.ReplicatedStorage.Fast.OnServerEvent:Connect(function() end)";
    let events = capture(place(server, client), Network::default(), 1, 90);
    let limited = events
        .iter()
        .filter(|e| matches!(e, Event::Fate(_, Fate::RateLimited)))
        .count();
    assert_eq!(limited, 100);
}

#[test]
fn netsim_losses_show_as_lost_and_resent() {
    let lossy = Way {
        latency: 0.05,
        jitter: 0.0,
        loss: 0.5,
    };
    let client = "task.wait(0.5) for i = 1, 40 do \
        game.ReplicatedStorage.Fast:FireServer(i) game.ReplicatedStorage.Ping:FireServer(i) task.wait() end";
    let server = "game.ReplicatedStorage.Fast.OnServerEvent:Connect(function() end) \
        game.ReplicatedStorage.Ping.OnServerEvent:Connect(function() end)";
    let network = Network {
        down: lossy,
        up: lossy,
    };
    let events = capture(place(server, client), network, 1, 240);
    let calls = calls(&events);
    let lost = calls
        .iter()
        .filter(|c| c.kind == Kind::Unreliable && fates(&events, c.id).contains(&Fate::Lost))
        .count();
    assert!((5..35).contains(&lost), "{lost} of 40 lost");
    let resent = calls
        .iter()
        .filter(|c| c.kind == Kind::Event)
        .filter(|c| {
            fates(&events, c.id)
                .iter()
                .any(|f| matches!(f, Fate::Resent(_)))
        })
        .count();
    assert!((5..35).contains(&resent), "{resent} of 40 resent");
}

#[test]
fn invocations_report_how_long_they_yielded_and_their_error() {
    let server = "local ask = game.ReplicatedStorage.Ask \
        ask.OnServerInvoke = function(_, slow) \
            if slow then task.wait(1.5) return 1 end \
            error('nope') \
        end";
    let client = "task.wait(0.5) local ask = game.ReplicatedStorage.Ask \
        ask:InvokeServer(true) pcall(ask.InvokeServer, ask, false)";
    let network = Network {
        down: Way {
            latency: 0.1,
            ..Way::default()
        },
        up: Way {
            latency: 0.1,
            ..Way::default()
        },
    };
    let events = capture(place(server, client), network, 1, 300);
    let replies: Vec<&Reply> = events
        .iter()
        .filter_map(|e| match e {
            Event::Replied(_, reply) => Some(reply),
            _ => None,
        })
        .collect();
    assert_eq!(replies.len(), 2, "{events:#?}");
    // 0.1 there, 1.5 waiting, 0.1 back (to the frame).
    assert!((replies[0].seconds - 1.7).abs() < 0.05, "{replies:?}");
    assert_eq!(replies[0].error, None);
    let error = replies[1].error.as_deref().expect("the error");
    assert!(error.contains("nope"), "{error}");
}
