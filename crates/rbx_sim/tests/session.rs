//! A server and its clients in one process: replication, remotes, several
//! local players, the character simulated by its own client.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::{conformance, Input, Peer, Session};

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
        let mut buffer = [0u8; 4096];
        let n = stream.read(&mut buffer).unwrap();
        let request = String::from_utf8_lossy(&buffer[..n]).into_owned();
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
    for _ in 0..240 {
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
