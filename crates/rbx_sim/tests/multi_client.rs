//! Studio's Server & Clients test: N local players in one session, each
//! with a client of its own, named and numbered as Studio's test players,
//! each peer printing to its own Output, and a recording of all of them
//! replaying exactly.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::replay::{Action, Replay};
use rbx_sim::{conformance, Input, Peer, Session};

fn service(dom: &WeakDom, class: &str) -> Ref {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
        .unwrap_or_else(|| panic!("no {class}"))
}

fn place(server: &str, client: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let sss = service(&dom, "ServerScriptService");
    let script = dom.new_instance("Script", "Server", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(server.into()));
    let starter_player = service(&dom, "StarterPlayer");
    let starter = dom.get(starter_player).unwrap().children()[0];
    let local = dom.new_instance("LocalScript", "Client", Some(starter));
    let _ = dom.set_property(local, "Source", Variant::String(client.into()));
    dom.take_changes();
    dom
}

fn printed(session: &mut Session, lines: &mut Vec<(Peer, String)>) {
    for (peer, output) in session.take_output() {
        lines.extend(output.lines().iter().map(|l| (peer, l.clone())));
    }
}

const SERVER: &str = "game:GetService('Players').PlayerAdded:Connect(function(p) \
     print('added', p.Name, p.UserId) end)";
const CLIENT: &str = "local me = game:GetService('Players').LocalPlayer \
     print('me', me.Name, me.UserId)";

#[test]
fn four_test_players_join_in_order_as_player1_to_player4() {
    let mut s = Session::new(place(SERVER, CLIENT)).unwrap();
    s.start();
    assert_eq!(s.join_test_players(4, None).unwrap(), vec![0, 1, 2, 3]);
    let mut lines = Vec::new();
    for _ in 0..60 {
        s.step();
        printed(&mut s, &mut lines);
    }
    let added: Vec<&str> = lines
        .iter()
        .filter(|(p, l)| *p == Peer::Server && l.starts_with("added"))
        .map(|(_, l)| l.as_str())
        .collect();
    assert_eq!(
        added,
        [
            "added Player1 -1",
            "added Player2 -2",
            "added Player3 -3",
            "added Player4 -4"
        ]
    );
    // Each client's line reaches its own Output, and no other.
    for i in 0..4 {
        let mine = format!("me Player{} -{}", i + 1, i + 1);
        let seen: Vec<Peer> = lines
            .iter()
            .filter(|(_, l)| *l == mine)
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(seen, [Peer::Client(i)], "{lines:#?}");
    }
    let names: Vec<&str> = s.clients.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Player1", "Player2", "Player3", "Player4"]);
}

#[test]
fn added_clients_number_on_after_one_leaves() {
    let mut s = Session::new(place(SERVER, CLIENT)).unwrap();
    s.start();
    s.join_test_players(2, None).unwrap();
    s.step();
    s.leave(0);
    s.step();
    assert_eq!(s.join_test_players(1, None).unwrap(), vec![1]);
    let mut lines = Vec::new();
    for _ in 0..30 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(
        lines.contains(&(Peer::Server, "added Player3 -3".into())),
        "{lines:#?}"
    );
    let names: Vec<&str> = s.clients.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Player2", "Player3"]);
}

#[test]
fn three_players_replay_with_zero_divergence() {
    // A place from a file, as the editor's are (one built in code doesn't
    // come back the same through the recording's XML).
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/smoke/obby.rbxlx");
    let mut s = Session::new(rbx_sim::load::read_place(&path).unwrap()).unwrap();
    s.record(3);
    s.start();
    s.join_test_players(3, None).unwrap();
    // Each player walks their own way, as three windows' keys would.
    for frame in 0..240u32 {
        for (i, key) in ["W", "A", "D"].into_iter().enumerate() {
            let down = match frame {
                30 => Some(true),
                150 => Some(false),
                _ => None,
            };
            if let Some(down) = down {
                s.act(
                    Peer::Client(i),
                    Action::Input(Input::Key {
                        code: key.into(),
                        down,
                    }),
                );
            }
        }
        if frame == 90 {
            s.act(Peer::Client(1), Action::Input(Input::Key {
                code: "Space".into(),
                down: true,
            }));
        }
        s.step();
    }
    let recording = s.recording().unwrap();
    for i in 0..3 {
        assert!(
            recording
                .actions
                .iter()
                .any(|l| l.peer == Peer::Client(i) && matches!(l.action, Action::Input(_))),
            "player {i}'s input is recorded"
        );
    }
    let mut replay = Replay::new(recording).unwrap();
    if let Err(divergence) = replay.run_to(u64::MAX) {
        panic!("{divergence}");
    }
    assert_eq!(replay.frame(), 240);
}
