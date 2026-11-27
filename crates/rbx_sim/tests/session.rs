//! A server and its clients in one process: replication, remotes, several
//! local players, the character simulated by its own client.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::{conformance, Input, Peer, Session};

mod common;

#[test]
fn tags_replicate_and_debris_and_player_helpers_work() {
    let mut s = session(
        place(
            "local p = Instance.new('Part') p.Name = 'Tagged' p.Anchored = true p:AddTag('Loot') p.Parent = workspace \
             task.wait(1) local q = Instance.new('Part') q.Name = 'Late' q.Anchored = true q.Parent = workspace \
             task.wait(0.5) q:AddTag('Loot') task.wait(0.5) p:RemoveTag('Loot') \
             local d = Instance.new('Folder') d.Name = 'Brief' d.Parent = workspace \
             game:GetService('Debris'):AddItem(d, 0.5) task.wait(1) print('debris gone', d.Parent == nil) \
             local player = game:GetService('Players'):GetPlayers()[1] \
             print('name', game:GetService('Players'):GetNameFromUserIdAsync(player.UserId)) \
             print('ping', player:GetNetworkPing(), player:IsFriendsWithAsync(1))",
            "local cs = game:GetService('CollectionService') \
             cs:GetInstanceAddedSignal('Loot'):Connect(function(i) print('client added', i.Name) end) \
             cs:GetInstanceRemovedSignal('Loot'):Connect(function(i) print('client removed', i.Name) end) \
             local t = workspace:WaitForChild('Tagged') print('client has', t:HasTag('Loot'), #cs:GetTagged('Loot'))",
        ),
        1,
    );
    let mut lines = Vec::new();
    for _ in 0..240 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(
        has(&lines, Peer::Client(0), "client has true 1"),
        "{lines:#?}"
    );
    assert!(
        has(&lines, Peer::Client(0), "client added Late"),
        "{lines:#?}"
    );
    assert!(
        has(&lines, Peer::Client(0), "client removed Tagged"),
        "{lines:#?}"
    );
    assert!(has(&lines, Peer::Server, "debris gone true"), "{lines:#?}");
    assert!(has(&lines, Peer::Server, "name Player1"), "{lines:#?}");
    assert!(has(&lines, Peer::Server, "ping 0 false"), "{lines:#?}");
}

#[test]
fn a_purchase_prompt_reaches_the_dialog_and_process_receipt() {
    let mut s = Session::new(place(
        "local mps = game:GetService('MarketplaceService') \
         mps.ProcessReceipt = function(info) print('receipt', info.ProductId) return Enum.ProductPurchaseDecision.PurchaseGranted end \
         mps.PromptGamePassPurchaseFinished:Connect(function(p, id, bought) print('pass', id, bought) \
           print('owns', mps:UserOwnsGamePassAsync(p.UserId, id)) end) \
         mps:BindReceiptHandler(Enum.ReceiptType.DeveloperProduct, function(info) print('bound', info.ProductId) \
           return Enum.ProductPurchaseDecision.PurchaseGranted end, { 99 })",
        "local mps = game:GetService('MarketplaceService') local me = game:GetService('Players').LocalPlayer \
         mps.PromptProductPurchaseFinished:Connect(function(user, id, bought) print('client product', id, bought) end) \
         task.wait(0.5) mps:PromptProductPurchase(me, 77) task.wait(0.5) mps:PromptGamePassPurchase(me, 88) \
         task.wait(0.5) mps:PromptProductPurchase(me, 99)",
    ))
    .unwrap();
    s.set_services(rbx_lua::server::Services {
        universe: Some(format!("purchase-test-{}", std::process::id())),
        ..Default::default()
    });
    s.start();
    s.join("Player1", 1, None).unwrap();
    let mut lines = Vec::new();
    let mut prompts = Vec::new();
    for _ in 0..180 {
        s.step();
        for prompt in s.take_prompts() {
            prompts.push((prompt.kind.clone(), prompt.id));
            s.purchase_finished(&prompt, true);
        }
        printed(&mut s, &mut lines);
    }
    assert_eq!(
        prompts,
        vec![
            ("Product".to_owned(), 77),
            ("GamePass".to_owned(), 88),
            ("Product".to_owned(), 99)
        ]
    );
    assert!(has(&lines, Peer::Server, "receipt 77"), "{lines:#?}");
    assert!(has(&lines, Peer::Server, "bound 99"), "{lines:#?}");
    assert!(!has(&lines, Peer::Server, "receipt 99"), "{lines:#?}");
    assert!(
        has(&lines, Peer::Client(0), "client product 77 true"),
        "{lines:#?}"
    );
    assert!(has(&lines, Peer::Server, "pass 88 true"), "{lines:#?}");
    assert!(has(&lines, Peer::Server, "owns true"), "{lines:#?}");
}

#[test]
fn datastores_persist_between_runs_of_a_place() {
    let dir = std::env::temp_dir().join(format!("rbx_sim_datastore_{}", std::process::id()));
    let file = dir.join("place.json");
    let run = |script: &str| {
        let mut s = Session::new(place(script, "")).unwrap();
        s.set_services(rbx_lua::server::Services {
            data_file: Some(file.clone()),
            ..Default::default()
        });
        s.start();
        let mut lines = Vec::new();
        for _ in 0..60 {
            s.step();
            printed(&mut s, &mut lines);
        }
        // The store object a script holds isn't a root of the place.
        let dom = s.server.dom();
        assert!(dom
            .root_refs()
            .iter()
            .all(|r| dom.get(*r).is_some_and(|i| i.name() != "Saves")));
        drop(dom);
        lines
    };
    run("game:GetService('DataStoreService'):GetDataStore('Saves'):SetAsync('k', { coins = 12 })");
    let lines = run(
        "print('loaded', game:GetService('DataStoreService'):GetDataStore('Saves'):GetAsync('k').coins)",
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(has(&lines, Peer::Server, "loaded 12"), "{lines:#?}");
}

/// HttpService against a local test server, with HttpEnabled off, and
/// offline (one test: the offline switch is the whole process's).
#[test]
fn http_requests_reach_a_local_server_and_fail_like_roblox_offline() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        // The whole request, body too: the client may send the body in a
        // write of its own, and closing with it unread resets the
        // connection (Windows does, before the client reads the answer).
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = stream.read(&mut buffer).unwrap();
            request.extend_from_slice(&buffer[..n]);
            let text = String::from_utf8_lossy(&request);
            let Some((head, body)) = text.split_once("\r\n\r\n") else {
                assert!(n > 0, "the request ended inside its headers");
                continue;
            };
            let length = head
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")?
                        .trim()
                        .parse()
                        .ok()
                })
                .unwrap_or(0);
            if n == 0 || body.len() >= length {
                break;
            }
        }
        let request = String::from_utf8_lossy(&request).into_owned();
        let body = r#"{"ok":true}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        request
    });
    let script = format!(
        "local http = game:GetService('HttpService') \
         print('off', select(2, pcall(http.GetAsync, http, 'http://127.0.0.1:{port}/'))) \
         http.HttpEnabled = true \
         local answer = http:RequestAsync({{ Url = 'http://127.0.0.1:{port}/save', Method = 'POST', Body = 'hi' }}) \
         print('answer', answer.StatusCode, http:JSONDecode(answer.Body).ok) \
         print('offline', select(2, pcall(http.GetAsync, http, 'https://example.com/')))"
    );
    rbx_net::set_forced(true);
    let mut s = session(place(&script, ""), 0);
    let mut lines = Vec::new();
    // The requests run on worker threads: wait on what they print, not
    // on a number of frames (a loaded machine needs many more).
    for _ in 0..20_000 {
        s.step();
        printed(&mut s, &mut lines);
        if lines.iter().any(|(_, l)| l.starts_with("offline")) {
            break;
        }
    }
    rbx_net::set_forced(false);
    let request = server.join().unwrap();
    assert!(request.starts_with("POST /save"), "{request}");
    assert!(
        has(
            &lines,
            Peer::Server,
            "off Http requests are not enabled. Enable via game settings"
        ),
        "{lines:#?}"
    );
    assert!(has(&lines, Peer::Server, "answer 200 true"), "{lines:#?}");
    assert!(
        has(&lines, Peer::Server, "offline HttpError: ConnectFail"),
        "{lines:#?}"
    );
}

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
        lines.extend(
            output
                .lines()
                .iter()
                .filter(|l| !common::animation_warning(l))
                .map(|l| (peer, l.clone())),
        );
    }
}

fn has(lines: &[(Peer, String)], peer: Peer, text: &str) -> bool {
    lines.iter().any(|(p, l)| *p == peer && l == text)
}

#[test]
fn a_place_without_a_players_service_still_takes_players() {
    let mut dom = place("", "");
    let players = service(&dom, "Players");
    dom.remove(players);
    let mut s = session(dom, 1);
    s.step();
    assert!(s.sim_mut(Peer::Client(0)).is_some());
}

#[test]
fn a_place_without_starter_player_plays_with_its_defaults() {
    let mut dom = place("", "");
    let starter_player = service(&dom, "StarterPlayer");
    dom.remove(starter_player);
    let sss = service(&dom, "ServerScriptService");
    let script = dom.new_instance("Script", "Check", Some(sss));
    let _ = dom.set_property(
        script,
        "Source",
        Variant::String(
            "local sp = game:GetService('StarterPlayer') \
             print('folders', sp:FindFirstChild('StarterPlayerScripts') ~= nil, sp:FindFirstChild('StarterCharacterScripts') ~= nil) \
             local p = game:GetService('Players').PlayerAdded:Wait() \
             print('camera', p.CameraMode, p.CameraMaxZoomDistance, p.CameraMinZoomDistance)"
                .into(),
        ),
    );
    let mut s = session(dom, 1);
    let mut lines = Vec::new();
    for _ in 0..30 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "folders true true"), "{lines:#?}");
    assert!(
        has(
            &lines,
            Peer::Server,
            "camera Enum.CameraMode.Classic 400 0.5"
        ),
        "{lines:#?}"
    );
}

/// Janitor-style alias loops add keys while they walk the table; on a live
/// server (sandbox probe) all 14 aliases land, 28 keys.
#[test]
fn a_loop_that_adds_keys_as_it_walks_sees_them_as_live() {
    let probe = "local t = {} \
         for _, k in {'Add','AddPromise','AddObject','Remove','Get','Cleanup','Destroy','LinkToInstance', \
           'LinkToInstances','Give','GivePromise','GiveObject','Clean','CurrentlyCleaning'} do t[k] = true end \
         for k, v in next, t do t[string.lower(string.sub(k, 1, 1)) .. string.sub(k, 2)] = v end \
         local n = 0 for _ in pairs(t) do n += 1 end print('keys', n, t.clean)";
    let mut s = session(place(probe, probe), 1);
    let mut lines = Vec::new();
    for _ in 0..10 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "keys 28 true"), "{lines:#?}");
    assert!(has(&lines, Peer::Client(0), "keys 28 true"), "{lines:#?}");
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
    root_position(session, peer)[2]
}

fn root_position(session: &Session, peer: Peer) -> [f32; 3] {
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
        Some(Variant::CFrame(cf)) => [cf.position.x, cf.position.y, cf.position.z],
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
    let mut dom = place(
        "task.wait(0.5) local a, b = unpack(game:GetService('Players'):GetPlayers()) \
         local ya = a.Character.HumanoidRootPart.Position.Y \
         local yb = b.Character.HumanoidRootPart.Position.Y \
         print('apart', math.abs(ya - yb) > 3)",
        "",
    );
    // A 6x6 spawn puts both at its centre (a bigger one picks random spots).
    let workspace = service(&dom, "Workspace");
    let spawn = dom
        .get(workspace)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).unwrap().class() == "SpawnLocation")
        .unwrap();
    let _ = dom.set_property(
        spawn,
        "Size",
        Variant::Vector3(rbx_dom::Vector3Data {
            x: 6.0,
            y: 1.0,
            z: 6.0,
        }),
    );
    dom.take_changes();
    let mut s = session(dom, 2);
    let mut lines = Vec::new();
    for _ in 0..60 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "apart true"), "{lines:#?}");
}

#[test]
fn the_server_sees_the_owner_s_walking_velocity() {
    let mut s = session(
        place(
            "local player = game:GetService('Players'):GetPlayers()[1] \
             or game:GetService('Players').PlayerAdded:Wait() \
             local character = player.Character or player.CharacterAdded:Wait() \
             task.wait(2) \
             character.Humanoid:MoveTo(character.HumanoidRootPart.Position + Vector3.new(0, 0, -60)) \
             task.wait(0.75) \
             print('server speed', math.floor(character.HumanoidRootPart.AssemblyLinearVelocity.Magnitude + 0.5))",
            "local player = game:GetService('Players').LocalPlayer \
             local character = player.Character or player.CharacterAdded:Wait() \
             task.wait(2.75) \
             print('client speed', math.floor(character.HumanoidRootPart.AssemblyLinearVelocity.Magnitude + 0.5))",
        ),
        1,
    );
    let mut lines = Vec::new();
    for _ in 0..300 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(
        has(&lines, Peer::Client(0), "client speed 16"),
        "{lines:#?}"
    );
    assert!(has(&lines, Peer::Server, "server speed 16"), "{lines:#?}");
}

#[test]
fn a_kick_removes_the_player_with_its_message() {
    let mut s = session(
        place(
            "game:GetService('Players').PlayerRemoving:Connect(function(p) print('removing', p.Name) end) \
             task.wait(1) game:GetService('Players').Player2:Kick('bye now')",
            "",
        ),
        2,
    );
    let mut lines = Vec::new();
    for _ in 0..120 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert_eq!(s.kicked, vec![("Player2".to_owned(), "bye now".to_owned())]);
    assert!(has(&lines, Peer::Server, "removing Player2"), "{lines:#?}");
    let dom = s.server.dom();
    let players = service(&dom, "Players");
    let left: Vec<&str> = dom
        .get(players)
        .unwrap()
        .children()
        .iter()
        .map(|r| dom.get(*r).unwrap().name())
        .collect();
    assert_eq!(left, vec!["Player1"]);
}

#[test]
fn a_backpack_tool_equips_and_welds_to_the_right_hand() {
    let mut dom = place(
        "task.wait(1) local player = game:GetService('Players').Player1 \
         local character = player.Character \
         local tool = player.Backpack:FindFirstChild('Sword') \
         tool.Equipped:Connect(function() print('equipped') end) \
         task.wait(0.5) character.Humanoid:EquipTool(tool) task.wait(0.2) \
         print('parent', tool.Parent == character, 'grip', character:FindFirstChild('RightGrip', true) ~= nil) \
         character.Humanoid:UnequipTools() task.wait(0.2) print('back', tool.Parent.Name)",
        "",
    );
    let pack = service(&dom, "StarterPack");
    let tool = dom.new_instance("Tool", "Sword", Some(pack));
    let handle = dom.new_instance("Part", "Handle", Some(tool));
    let _ = dom.set_property(handle, "Anchored", Variant::Bool(false));
    dom.take_changes();
    let mut s = session(dom, 1);
    let mut lines = Vec::new();
    for _ in 0..180 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "equipped"), "{lines:#?}");
    assert!(
        has(&lines, Peer::Server, "parent true grip true"),
        "{lines:#?}"
    );
    assert!(has(&lines, Peer::Server, "back Backpack"), "{lines:#?}");
}

#[test]
fn auto_assignable_teams_fill_evenly() {
    let mut dom = place(
        "task.wait(1) for _, p in game:GetService('Players'):GetPlayers() do \
         print('team', p.Team and p.Team.Name) end",
        "",
    );
    let teams = service(&dom, "Teams");
    for (name, color) in [("Red", 21u32), ("Blue", 23)] {
        let team = dom.new_instance("Team", name, Some(teams));
        let _ = dom.set_property(team, "TeamColor", Variant::BrickColor(color));
    }
    dom.take_changes();
    let mut s = session(dom, 2);
    let mut lines = Vec::new();
    for _ in 0..90 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "team Red"), "{lines:#?}");
    assert!(has(&lines, Peer::Server, "team Blue"), "{lines:#?}");
}

#[test]
fn number_keys_switch_between_tools() {
    let mut dom = place(
        "",
        "local player = game:GetService('Players').LocalPlayer \
         while true do task.wait(0.25) local c = player.Character \
         local t = c and c:FindFirstChildOfClass('Tool') print('holding', t and t.Name) end",
    );
    let pack = service(&dom, "StarterPack");
    for name in ["Sword", "Torch"] {
        let tool = dom.new_instance("Tool", name, Some(pack));
        let handle = dom.new_instance("Part", "Handle", Some(tool));
        let _ = dom.set_property(handle, "Anchored", Variant::Bool(false));
    }
    dom.take_changes();
    let mut s = session(dom, 1);
    let mut lines = Vec::new();
    let tap = |s: &mut Session, code: &str| {
        for down in [true, false] {
            s.input(
                0,
                Input::Key {
                    code: code.into(),
                    down,
                },
            );
        }
    };
    for frame in 0..240 {
        match frame {
            60 => tap(&mut s, "One"),
            120 => tap(&mut s, "Two"),
            180 => tap(&mut s, "Two"),
            _ => {}
        }
        s.step();
        printed(&mut s, &mut lines);
    }
    let held: Vec<&str> = lines
        .iter()
        .filter(|(p, _)| *p == Peer::Client(0))
        .map(|(_, l)| l.as_str())
        .collect();
    let at = |i: usize| held.get(i).copied().unwrap_or("");
    // Printed every 15 frames: before 60 nothing, 60..120 the sword,
    // 120..180 the torch, after 180 nothing again.
    assert_eq!(at(5), "holding Sword", "{held:#?}");
    assert_eq!(at(9), "holding Torch", "{held:#?}");
    assert_eq!(at(14), "holding nil", "{held:#?}");
}

#[test]
fn terrain_a_server_script_builds_holds_the_client_character_up() {
    // The client meets Terrain with the place, voxels already in it; the
    // character (and a crate near it) is simulated there.
    let mut s = session(
        place(
            "local terrain = workspace.Terrain \
             terrain:FillBlock(CFrame.new(0, -12, 0), Vector3.new(256, 24, 256), Enum.Material.Grass) \
             workspace.Baseplate:Destroy() \
             task.wait(0.5) \
             workspace.SpawnLocation:Destroy() \
             local crate = Instance.new('Part') \
             crate.Position = Vector3.new(6, 10, 0) \
             crate.Parent = workspace \
             task.wait(2) \
             print('crate', math.floor(crate.Position.Y * 10 + 0.5) / 10)",
            "",
        ),
        1,
    );
    let mut lines = Vec::new();
    for _ in 0..240 {
        s.step();
        printed(&mut s, &mut lines);
    }
    for peer in [Peer::Server, Peer::Client(0)] {
        let y = root_position(&s, peer)[1];
        assert!(y > 3.0 && y < 6.0, "{peer:?}: the root fell to {y}");
    }
    assert!(has(&lines, Peer::Server, "crate 2.6"), "{lines:#?}");
}

#[test]
fn a_teleport_into_the_same_place_carries_its_data_and_gui() {
    let mut s = session(
        place(
            "",
            "local ts = game:GetService('TeleportService') \
             local gui = Instance.new('ScreenGui') gui.Name = 'Loading' \
             Instance.new('Frame', gui).Name = 'Bar' \
             task.wait(0.5) ts:Teleport(game.PlaceId, nil, { level = 7 }, gui) \
             ts:Teleport(424242, nil, {}) \
             ts.TeleportInitFailed:Connect(function(_, result) print('failed', result.Name) end)",
        ),
        1,
    );
    let mut lines = Vec::new();
    let mut teleports = Vec::new();
    for _ in 0..90 {
        s.step();
        teleports.extend(s.take_teleports());
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Client(0), "failed Failure"), "{lines:#?}");
    assert_eq!(teleports.len(), 1);
    let teleport = teleports.remove(0);
    assert_eq!(teleport.request.data.as_deref(), Some(r#"{"level":7}"#));
    assert_eq!(teleport.request.players, vec![s.clients[0].player]);
    let gui = teleport.gui.expect("the teleport GUI comes along");

    let mut arrived = Session::new(place(
        "game:GetService('Players').PlayerAdded:Connect(function(p) \
           print('join', p:GetJoinData().TeleportData.level, p:GetJoinData().SourcePlaceId) end)",
        "local ts = game:GetService('TeleportService') \
         ts.LocalPlayerArrivedFromTeleport:Connect(function(g, data) print('arrived', g.Name, data.level) end) \
         print('data', ts:GetLocalPlayerTeleportData().level, ts:GetArrivingTeleportGui().Bar.ClassName)",
    ))
    .unwrap();
    let mut services = rbx_lua::server::Services::default();
    services.arrivals.insert(
        -1,
        rbx_lua::server::Arrival {
            data: teleport.request.data.clone(),
            source_place: 5,
            gui: None,
        },
    );
    arrived.set_services(services);
    arrived.set_arriving_gui(-1, gui);
    arrived.start();
    arrived.join("Player1", -1, None).unwrap();
    let mut lines = Vec::new();
    for _ in 0..30 {
        arrived.step();
        printed(&mut arrived, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "join 7 5"), "{lines:#?}");
    assert!(has(&lines, Peer::Client(0), "data 7 Frame"), "{lines:#?}");
    assert!(
        has(&lines, Peer::Client(0), "arrived Loading 7"),
        "{lines:#?}"
    );
}

#[test]
fn a_badge_award_lands_in_the_ledger_with_a_toast() {
    let mut s = session(
        place(
            "local bs = game:GetService('BadgeService') local p = game:GetService('Players'):GetPlayers()[1] \
             while not p do task.wait() p = game:GetService('Players'):GetPlayers()[1] end \
             print('award', bs:AwardBadgeAsync(p.UserId, 1)) print('again', bs:AwardBadgeAsync(p.UserId, 1)) \
             print('has', bs:UserHasBadgeAsync(p.UserId, 1), #bs:CheckUserBadgesAsync(p.UserId, { 1, 2 }))",
            "print('client', pcall(function() return game:GetService('BadgeService'):UserHasBadgeAsync(1, 1) end))",
        ),
        1,
    );
    let mut lines = Vec::new();
    let mut notices = Vec::new();
    // The badge's name comes from the web: wait on
    // the last line, not on a number of frames.
    for _ in 0..20_000 {
        s.step();
        notices.extend(s.take_notices());
        printed(&mut s, &mut lines);
        if has(&lines, Peer::Server, "has true 1") {
            break;
        }
    }
    assert!(has(&lines, Peer::Server, "award true"), "{lines:#?}");
    assert!(has(&lines, Peer::Server, "again false"), "{lines:#?}");
    assert!(has(&lines, Peer::Server, "has true 1"), "{lines:#?}");
    assert!(
        has(&lines, Peer::Client(0), "client true false"),
        "{lines:#?}"
    );
    assert_eq!(notices.len(), 1, "{notices:#?}");
    assert_eq!(notices[0].title, "Badge Awarded");
    assert_eq!(notices[0].player, Some(s.clients[0].player));
}

#[test]
fn a_touch_device_gets_context_action_buttons() {
    let mut s = Session::new(place(
        "",
        "local cas = game:GetService('ContextActionService') \
         cas:BindAction('Boost', function(_, state) print('boost', state.Name) end, true, Enum.KeyCode.B) \
         cas:SetTitle('Boost', 'Go') \
         local button = cas:GetButton('Boost') \
         print('button', button.Parent.Parent.Name, button.ActionTitle.Text, \
           game:GetService('UserInputService').TouchEnabled) \
         task.wait(0.2) print('at', button.AbsolutePosition.X + 35, button.AbsolutePosition.Y + 35)",
    ))
    .unwrap();
    s.set_services(rbx_lua::server::Services {
        device: rbx_lua::server::Device {
            touch: true,
            keyboard: false,
            mouse: false,
            gamepad: false,
        },
        ..Default::default()
    });
    s.start();
    s.join("Player1", -1, None).unwrap();
    // A phone's home-bar side: the buttons keep clear of it, as drawn.
    s.clients[0].sim.set_safe_insets([0.0, 0.0, 60.0, 0.0]);
    let mut lines = Vec::new();
    let mut at = None;
    for _ in 0..90 {
        s.step();
        printed(&mut s, &mut lines);
        if at.is_none() {
            at = lines.iter().find_map(|(_, l)| {
                let mut words = l.strip_prefix("at ")?.split(' ');
                Some([
                    words.next()?.parse::<f32>().ok()?,
                    words.next()?.parse::<f32>().ok()?,
                ])
            });
            if let Some([x, y]) = at {
                let position = [x, y + 58.0];
                s.input(
                    0,
                    Input::Move {
                        position,
                        delta: [0.0, 0.0],
                    },
                );
                s.input(
                    0,
                    Input::Button {
                        button: 1,
                        down: true,
                    },
                );
            }
        }
    }
    assert!(
        has(&lines, Peer::Client(0), "button ContextActionGui Go true"),
        "{lines:#?}"
    );
    assert!(has(&lines, Peer::Client(0), "boost Begin"), "{lines:#?}");
    assert_eq!(at.map(|[x, _]| x), Some(800.0 - 60.0 - 95.0 + 35.0));
}

#[test]
fn a_drag_detector_crate_and_a_ui_drag_detector_follow_the_mouse() {
    let mut s = session(
        place(
            "local crate = Instance.new('Part') crate.Name = 'Crate' crate.Anchored = true \
             crate.Size = Vector3.new(4, 4, 4) crate.Position = Vector3.new(10, 5, -10) \
             local drag = Instance.new('DragDetector') drag.Parent = crate crate.Parent = workspace \
             drag.DragStart:Connect(function(player, ray, view, hit, part) print('start', player.Name, part.Name) end) \
             drag.DragEnd:Connect(function(player) print('end', player.Name, math.round(crate.Position.Z)) end)",
            "local camera = workspace.CurrentCamera task.wait(0.5) camera.CameraType = Enum.CameraType.Scriptable \
             camera.CFrame = CFrame.lookAt(Vector3.new(10, 15, 10), Vector3.new(10, 5, -10)) \
             local gui = Instance.new('ScreenGui') local frame = Instance.new('Frame') \
             frame.Position = UDim2.fromOffset(100, 100) frame.Size = UDim2.fromOffset(100, 100) \
             local ui = Instance.new('UIDragDetector') ui.Parent = frame frame.Parent = gui \
             ui.DragEnd:Connect(function() print('ui', frame.Position.X.Offset, frame.Position.Y.Offset) end) \
             gui.Parent = game:GetService('Players').LocalPlayer:WaitForChild('PlayerGui') \
             task.wait(0.2) local v, on = camera:WorldToViewportPoint(Vector3.new(10, 5, -10)) \
             local back = camera:ViewportPointToRay(v.X, v.Y) \
             print('vp', math.round(v.X), math.round(v.Y), on, math.round(back.Direction.Z * 100))",
        ),
        1,
    );
    if let Some(sim) = s.sim_mut(Peer::Client(0)) {
        sim.set_viewport([800.0, 600.0]);
    }
    let mut lines = Vec::new();
    for _ in 0..90 {
        s.step();
        printed(&mut s, &mut lines);
    }
    let gesture = |s: &mut Session, from: [f32; 2], to: [f32; 2]| {
        s.input(
            0,
            Input::Move {
                position: from,
                delta: [0.0, 0.0],
            },
        );
        s.step();
        s.input(
            0,
            Input::Button {
                button: 1,
                down: true,
            },
        );
        s.step();
        for i in 1..=4 {
            let t = i as f32 / 4.0;
            let at = [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ];
            s.input(
                0,
                Input::Move {
                    position: at,
                    delta: [0.0, 0.0],
                },
            );
            s.step();
        }
        s.input(
            0,
            Input::Button {
                button: 1,
                down: false,
            },
        );
        for _ in 0..10 {
            s.step();
        }
    };
    // Up the screen on the horizontal plane: the crate goes away (-Z).
    gesture(&mut s, [400.0, 300.0], [400.0, 260.0]);
    gesture(&mut s, [150.0, 208.0], [190.0, 228.0]);
    printed(&mut s, &mut lines);
    assert!(
        has(&lines, Peer::Server, "start Player1 Crate"),
        "{lines:#?}"
    );
    let ended = lines
        .iter()
        .find_map(|(p, l)| {
            (*p == Peer::Server)
                .then(|| l.strip_prefix("end Player1 "))
                .flatten()
        })
        .and_then(|z| z.parse::<f32>().ok())
        .unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(ended < -11.0, "{lines:#?}");
    assert!(has(&lines, Peer::Client(0), "ui 140 120"), "{lines:#?}");
    assert!(
        has(&lines, Peer::Client(0), "vp 400 300 true -89"),
        "{lines:#?}"
    );
}

#[test]
fn the_client_joins_after_the_servers_first_frames() {
    let mut s = Session::new(place(
        "task.delay(0.1, function() workspace:SetAttribute('Mark', true) end) \
             local p = game:GetService('Players').PlayerAdded:Wait() \
             print('joined', time() >= 1.5)",
        "print('client sees', workspace:GetAttribute('Mark'))",
    ))
    .unwrap();
    s.set_join_after(rbx_sim::JOIN_AFTER);
    s.start();
    s.join("Player1", 1, None).unwrap();
    let mut lines = Vec::new();
    for _ in 0..30 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "joined true"), "{lines:#?}");
    assert!(
        has(&lines, Peer::Client(0), "client sees true"),
        "{lines:#?}"
    );
}

#[test]
fn a_join_later_does_not_run_the_server_and_lands_on_the_joins_frame() {
    let session = || {
        let mut s = Session::new(place(
            "game:GetService('Players').PlayerAdded:Connect(function(p) \
               print('joined', p.Name, time()) end)",
            "print('client at', workspace.DistributedGameTime)",
        ))
        .unwrap();
        s.set_join_after(rbx_sim::JOIN_AFTER);
        s.start();
        s
    };
    let mut waited = session();
    waited.join("Player1", 1, None).unwrap();
    waited.join("Player2", 2, None).unwrap();
    let joined_on = waited.server.frame();

    // The editor's path: queuing returns before the server ran a frame.
    let mut queued = session();
    queued.join_later("Player1", 1, None);
    queued.join_later("Player2", 2, None);
    assert_eq!(queued.server.frame(), 0);
    assert!(queued.clients.is_empty() && queued.joining());
    while queued.joining() {
        queued.step();
    }
    assert!(queued.take_join_failures().is_empty());
    assert_eq!(queued.clients.len(), 2);
    assert_eq!(queued.server.frame(), joined_on + 1);

    waited.step();
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for _ in 0..30 {
        waited.step();
        queued.step();
        printed(&mut waited, &mut a);
        printed(&mut queued, &mut b);
    }
    for who in ["joined Player1", "joined Player2"] {
        assert!(
            a.iter()
                .any(|(p, l)| *p == Peer::Server && l.starts_with(who)),
            "{a:#?}"
        );
    }
    assert!(has(&a, Peer::Client(1), "client at 1.6"), "{a:#?}");
    assert_eq!(a, b);
}

#[test]
fn a_tool_given_on_character_added_stays_in_the_backpack() {
    let mut dom = place(
        "local tool = game:GetService('ServerScriptService'):WaitForChild('Kept') \
         game:GetService('Players').PlayerAdded:Connect(function(player) \
           player.CharacterAdded:Connect(function() tool.Parent = player.Backpack end) end)",
        "local bp = game:GetService('Players').LocalPlayer:WaitForChild('Backpack') \
         local t = bp:WaitForChild('Kept', 3) print('client has', t ~= nil, t and t:FindFirstChild('Handle') ~= nil)",
    );
    let sss = service(&dom, "ServerScriptService");
    let tool = dom.new_instance("Tool", "Kept", Some(sss));
    dom.new_instance("Part", "Handle", Some(tool));
    dom.take_changes();
    let mut s = session(dom, 1);
    let mut lines = Vec::new();
    for _ in 0..240 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(
        has(&lines, Peer::Client(0), "client has true true"),
        "{lines:#?}"
    );
}

/// Numbers from Studio's Play with the Orthographic Camera beta on, a
/// 759x397 viewport and the camera 40 studs back from the pillars.
#[test]
fn the_play_camera_projects_like_studio_in_every_orthographic_mode() {
    rbx_lua::set_orthographic_camera(true);
    let mut s = session(
        place(
            "",
            "local c = workspace.CurrentCamera task.wait(0.5) \
             local function r(v) return math.round(v * 100) / 100 end \
             print('fov', r(c.DiagonalFieldOfView), r(c.MaxAxisFieldOfView)) \
             c.DiagonalFieldOfView = 90 print('diag', r(c.FieldOfView)) \
             c.MaxAxisFieldOfView = 90 print('max', r(c.FieldOfView)) c.FieldOfView = 70 \
             c.CameraType = Enum.CameraType.Scriptable \
             c.CFrame = CFrame.lookAt(Vector3.new(0, 5, 40), Vector3.new(0, 5, 0)) \
             local v = c:WorldToViewportPoint(Vector3.new(10, 10, 0)) \
             local ray = c:ViewportPointToRay(c.ViewportSize.X * 0.75, c.ViewportSize.Y * 0.25, 5) \
             print('P', r(v.X), r(v.Y), r(v.Z), r(ray.Origin.X), r(ray.Origin.Z)) \
             c.ProjectionType = Enum.ProjectionType.Orthographic c.OrthographicSize = 20 \
             for _, mode in { 'Vertical', 'Horizontal', 'Diagonal', 'MaxAxis' } do \
               c.OrthographicMode = Enum.OrthographicMode[mode] \
               local v, on = c:WorldToViewportPoint(Vector3.new(10, 10, 0)) \
               local ray = c:ScreenPointToRay(c.ViewportSize.X * 0.75, c.ViewportSize.Y * 0.25, 5) \
               print(mode, r(v.X), r(v.Y), r(v.Z), on, r(ray.Origin.X), r(ray.Origin.Y), r(ray.Origin.Z), r(ray.Direction.Z)) \
             end",
        ),
        1,
    );
    if let Some(sim) = s.sim_mut(Peer::Client(0)) {
        sim.set_viewport([759.0, 397.0]);
    }
    let mut lines = Vec::new();
    for _ in 0..60 {
        s.step();
        printed(&mut s, &mut lines);
    }
    let me = Peer::Client(0);
    for expected in [
        "fov 132.68 106.48",
        "diag 49.73",
        "max 55.22",
        "P 450.37 163.06 40 2.74 35.91",
        "Vertical 578 99.25 40 true 9.56 7.08 34.9 -1",
        "Horizontal 759 8.75 40 true 5 6.09 34.9 -1",
        "Diagonal 807.78 -15.64 40 false 4.43 5.96 34.9 -1",
        "MaxAxis 759 8.75 40 true 5 6.09 34.9 -1",
    ] {
        assert!(has(&lines, me, expected), "{expected}: {lines:#?}");
    }
}

/// Studio's Stop, measured: `PlayerRemoving` sees the character still in
/// Workspace with its parts, `BindToClose` still counts the player, then
/// the character leaves the world whole (parent nil, not destroyed), and a
/// loop reading the character's parts never finds one gone.
#[test]
fn stop_tears_down_in_studio_s_order() {
    let mut s = session(
        place(
            "local Players = game:GetService('Players') \
             local function watch(c) task.spawn(function() while true do \
               if not pcall(function() return c.HumanoidRootPart end) then print('lost root') break end \
               game:GetService('RunService').Heartbeat:Wait() end end) end \
             for _, p in Players:GetPlayers() do if p.Character then watch(p.Character) end p.CharacterAdded:Connect(watch) end \
             Players.PlayerAdded:Connect(function(p) p.CharacterAdded:Connect(watch) end) \
             Players.PlayerRemoving:Connect(function(p) local c = p.Character \
               print('removing', c ~= nil and c.Parent == workspace and c:FindFirstChild('HumanoidRootPart') ~= nil) end) \
             game:BindToClose(function() print('closing', #Players:GetPlayers()) task.wait(0.1) end) \
             local function gone(c) c.AncestryChanged:Connect(function(_, parent) \
               if parent == nil then print('left', c:FindFirstChild('HumanoidRootPart') ~= nil) end end) end \
             for _, p in Players:GetPlayers() do if p.Character then gone(p.Character) end p.CharacterAdded:Connect(gone) end \
             Players.PlayerAdded:Connect(function(p) p.CharacterAdded:Connect(gone) end)",
            "",
        ),
        1,
    );
    let mut lines = Vec::new();
    for _ in 0..120 {
        s.step();
        printed(&mut s, &mut lines);
    }
    s.shutdown();
    printed(&mut s, &mut lines);
    let removing = lines.iter().position(|(_, l)| l == "removing true");
    let closing = lines.iter().position(|(_, l)| l == "closing 1");
    let left = lines.iter().position(|(_, l)| l == "left true");
    assert!(removing.is_some() && removing < closing, "{lines:#?}");
    assert!(closing.is_some() && closing < left, "{lines:#?}");
    assert!(!has(&lines, Peer::Server, "lost root"), "{lines:#?}");
}

/// The Command Bar during Play runs in the peer it is sent to, as Studio's
/// does in the server's or the focused client's DataModel.
#[test]
fn a_command_runs_in_the_peer_it_is_sent_to() {
    use rbx_sim::replay::Action;
    let mut s = session(place("", ""), 1);
    for _ in 0..60 {
        s.step();
    }
    s.act(
        Peer::Server,
        Action::Command(
            "print('server', game:GetService('RunService'):IsServer(), #game.Players:GetPlayers())"
                .into(),
        ),
    );
    s.act(
        Peer::Client(0),
        Action::Command("print('client', game.Players.LocalPlayer.Name) error('boom')".into()),
    );
    let mut lines = Vec::new();
    for _ in 0..5 {
        s.step();
        printed(&mut s, &mut lines);
    }
    assert!(has(&lines, Peer::Server, "server true 1"), "{lines:#?}");
    assert!(has(&lines, Peer::Client(0), "client Player1"), "{lines:#?}");
    assert!(
        lines
            .iter()
            .any(|(p, l)| *p == Peer::Client(0) && l.contains("CommandBar:1: boom")),
        "{lines:#?}"
    );
}
