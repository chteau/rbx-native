//! A server and its clients in one process: replication, remotes, several
//! local players, the character simulated by its own client.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::{conformance, Input, Peer, Session};

fn service(dom: &WeakDom, class: &str) -> Ref {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
        .unwrap_or_else(|| panic!("no {class}"))
}

/// A bare place with a server Script and a LocalScript in
/// StarterPlayerScripts.
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
    dom.take_changes();
    dom
}

fn session(dom: WeakDom, players: usize) -> Session {
    let mut session = Session::new(dom).expect("the place runs");
    session.start();
    for n in 1..=players {
        session
            .join(&format!("Player{n}"), -(n as i64), None)
            .expect("joins");
    }
    session
}

/// Every line printed so far, per peer.
fn printed(session: &mut Session, lines: &mut Vec<(Peer, String)>) {
    for (peer, output) in session.take_output() {
        lines.extend(output.lines().iter().map(|l| (peer, l.clone())));
    }
}

fn has(lines: &[(Peer, String)], peer: Peer, text: &str) -> bool {
    lines.iter().any(|(p, l)| *p == peer && l == text)
}

#[test]
fn each_player_has_a_client_of_their_own() {
    let mut s = session(
        place(
            "task.wait(1) print('players', #game:GetService('Players'):GetPlayers()) \
             print('server local', game:GetService('Players').LocalPlayer)",
            "print('me', game:GetService('Players').LocalPlayer.Name) \
             print('client', game:GetService('RunService'):IsClient())",
        ),
        2,
    );
    let mut lines = Vec::new();
    for _ in 0..120 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "players 2"), "{lines:#?}");
    assert!(has(&lines, Peer::Server, "server local nil"), "{lines:#?}");
    assert!(has(&lines, Peer::Client(0), "me Player1"), "{lines:#?}");
    assert!(has(&lines, Peer::Client(1), "me Player2"), "{lines:#?}");
    assert!(has(&lines, Peer::Client(1), "client true"), "{lines:#?}");
}

#[test]
fn a_remote_event_goes_both_ways() {
    let mut s = session(
        place(
            "local ping = game.ReplicatedStorage.Ping \
             ping.OnServerEvent:Connect(function(player, n, t) \
               print('server got', player.Name, n, t.k) ping:FireClient(player, n + 1) end)",
            "local ping = game.ReplicatedStorage.Ping \
             ping.OnClientEvent:Connect(function(n) print('client got', n) end) \
             ping:FireServer(41, { k = 'v' })",
        ),
        1,
    );
    let mut lines = Vec::new();
    for _ in 0..60 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(
        has(&lines, Peer::Server, "server got Player1 41 v"),
        "{lines:#?}"
    );
    assert!(has(&lines, Peer::Client(0), "client got 42"), "{lines:#?}");
}

#[test]
fn the_server_replicates_and_the_client_does_not() {
    let mut s = session(
        place(
            "local f = Instance.new('Folder') f.Name = 'FromServer' f.Parent = workspace \
             local hidden = Instance.new('Folder') hidden.Name = 'Hidden' \
             hidden.Parent = game:GetService('ServerStorage') \
             task.wait(2) print('server sees client part', workspace:FindFirstChild('FromClient') ~= nil)",
            "local f = workspace:WaitForChild('FromServer') print('client sees', f.Name) \
             print('client sees hidden', #game:GetService('ServerStorage'):GetChildren()) \
             local p = Instance.new('Part') p.Name = 'FromClient' p.Anchored = true p.Parent = workspace",
        ),
        1,
    );
    let mut lines = Vec::new();
    for _ in 0..180 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(
        has(&lines, Peer::Client(0), "client sees FromServer"),
        "{lines:#?}"
    );
    assert!(
        has(&lines, Peer::Client(0), "client sees hidden 0"),
        "{lines:#?}"
    );
    assert!(
        has(&lines, Peer::Server, "server sees client part false"),
        "{lines:#?}"
    );
}

fn root_z(session: &Session, peer: Peer) -> f32 {
    let sim = session.sim(peer).unwrap();
    let (player, _) = session.clients[0].sim.local_player().unwrap();
    let dom = sim.dom();
    let character = match dom.get(player).unwrap().properties().get("Character") {
        Some(Variant::Ref(c)) => *c,
        _ => panic!("no character"),
    };
    let root = dom
        .get(character)
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
}

#[test]
fn the_client_walks_its_character_and_the_server_follows() {
    let mut s = session(place("", ""), 1);
    s.run_for(1.5);
    let (server_before, client_before) = (root_z(&s, Peer::Server), root_z(&s, Peer::Client(0)));
    s.input(
        0,
        Input::Key {
            code: "W".into(),
            down: true,
        },
    );
    s.run_for(1.0);
    let (server_after, client_after) = (root_z(&s, Peer::Server), root_z(&s, Peer::Client(0)));
    assert!(
        client_after < client_before - 5.0,
        "{client_before} -> {client_after}"
    );
    assert!(
        (server_after - client_after).abs() < 1.0,
        "server {server_after}, client {client_after}"
    );
    assert!(server_after < server_before - 5.0);
}

#[test]
fn a_second_player_spawns_on_top_of_the_first() {
    let mut s = session(
        place(
            "task.wait(0.5) local a, b = unpack(game:GetService('Players'):GetPlayers()) \
             local ya = a.Character.HumanoidRootPart.Position.Y \
             local yb = b.Character.HumanoidRootPart.Position.Y \
             print('apart', math.abs(ya - yb) > 3)",
            "",
        ),
        2,
    );
    let mut lines = Vec::new();
    for _ in 0..60 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "apart true"), "{lines:#?}");
}
