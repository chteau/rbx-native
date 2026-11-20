//! What real games ran into in Play (local) that a live server and client
//! do otherwise (M17's corpus), each as our own minimal repro.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::{conformance, Peer, Session};

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
    dom.take_changes();
    dom
}

fn started(dom: WeakDom, services: rbx_lua::server::Services) -> Session {
    let mut session = Session::new(dom).expect("the place runs");
    session.set_services(services);
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session
}

fn steps(session: &mut Session, seconds: f64, lines: &mut Vec<(Peer, String)>) {
    for _ in 0..(seconds * rbx_sim::FRAME_HZ) as usize {
        session.step();
        collect(session, lines);
    }
}

fn collect(session: &mut Session, lines: &mut Vec<(Peer, String)>) {
    for (peer, output) in session.take_output() {
        lines.extend(output.lines().iter().map(|l| (peer, l.clone())));
    }
}

/// Runs `seconds` with one player and returns every line, per peer.
fn run(dom: WeakDom, seconds: f64) -> Vec<(Peer, String)> {
    let mut session = started(dom, Default::default());
    let mut lines = Vec::new();
    steps(&mut session, seconds, &mut lines);
    lines
}

fn has(lines: &[(Peer, String)], peer: Peer, text: &str) -> bool {
    lines.iter().any(|(p, l)| *p == peer && l == text)
}

const C: Peer = Peer::Client(0);
const S: Peer = Peer::Server;

#[test]
fn the_player_module_s_settings_are_there() {
    let lines = run(
        place(
            "",
            "local p = game:GetService('Players').LocalPlayer \
             print('touch', p.DevTouchMovementMode.Name, p.DevTouchCameraMode.Name) \
             local inset = game:GetService('GuiService').TopbarInset \
             print('inset', inset.Height > 0, inset.Min.X > 0) \
             local gs = UserSettings():GetService('UserGameSettings') \
             print('settings', gs.ClassName, UserSettings().GameSettings == gs, gs:InFullScreen()) \
             gs:SetCameraYInvertVisible() \
             local ps = p:WaitForChild('PlayerScripts') \
             local got ps.TouchMovementModeRegistered:Connect(function(m) got = m end) \
             ps:RegisterTouchMovementMode(Enum.TouchMovementMode.Thumbstick) \
             print('modes', #ps:GetRegisteredTouchMovementModes(), got == Enum.TouchMovementMode.Thumbstick)",
        ),
        0.5,
    );
    assert!(has(&lines, C, "touch UserChoice UserChoice"), "{lines:#?}");
    assert!(has(&lines, C, "inset true true"), "{lines:#?}");
    assert!(
        has(&lines, C, "settings UserGameSettings true false"),
        "{lines:#?}"
    );
    assert!(has(&lines, C, "modes 1 true"), "{lines:#?}");
}

#[test]
fn a_number_written_to_name_is_its_text() {
    let lines = run(
        place(
            "local f = Instance.new('Folder') f.Name = 12 print('name', f.Name, typeof(f.Name))",
            "",
        ),
        0.2,
    );
    assert!(has(&lines, S, "name 12 string"), "{lines:#?}");
}

#[test]
fn game_place_id_is_the_published_place() {
    let mut session = started(
        place(
            "print('ids', game.PlaceId, game.GameId)",
            "print('ids', game.PlaceId, game.GameId)",
        ),
        rbx_lua::server::Services {
            place_id: 1234,
            game_id: 99,
            ..Default::default()
        },
    );
    let mut lines = Vec::new();
    steps(&mut session, 0.3, &mut lines);
    assert!(has(&lines, S, "ids 1234 99"), "{lines:#?}");
    assert!(has(&lines, C, "ids 1234 99"), "{lines:#?}");
}

#[test]
fn bind_to_close_runs_when_the_server_shuts_down() {
    let mut session = started(
        place(
            "game:BindToClose(function() task.wait(1) print('closed', #game:GetService('Players'):GetPlayers()) end)",
            "print('client', pcall(game.BindToClose, game, function() end))",
        ),
        Default::default(),
    );
    let mut lines = Vec::new();
    steps(&mut session, 0.3, &mut lines);
    session.shutdown();
    collect(&mut session, &mut lines);
    assert!(has(&lines, S, "closed 0"), "{lines:#?}");
    assert!(
        has(
            &lines,
            C,
            "client false BindToClose can only be called on the server."
        ),
        "{lines:#?}"
    );
}

#[test]
fn collision_groups_the_server_registers_reach_the_client() {
    let lines = run(
        place(
            "game:GetService('PhysicsService'):RegisterCollisionGroup('Ghosts')",
            "task.wait(0.2) local p = Instance.new('Part') \
             print('set', pcall(function() p.CollisionGroup = 'Ghosts' end)) \
             print('registered', game:GetService('PhysicsService'):IsCollisionGroupRegistered('Ghosts'))",
        ),
        0.5,
    );
    assert!(has(&lines, C, "set true"), "{lines:#?}");
    assert!(has(&lines, C, "registered true"), "{lines:#?}");
}

#[test]
fn a_runaway_thread_dies_alone() {
    let lines = run(
        place(
            "task.spawn(function() while true do end end) \
             task.spawn(function() task.wait(0.1) print('still running') end)",
            "",
        ),
        0.5,
    );
    assert!(has(&lines, S, "still running"), "{lines:#?}");
}

#[test]
fn joints_and_assemblies_answer() {
    let lines = run(
        place(
            "local a = Instance.new('Part') a.Name = 'A' a.Anchored = true a.Size = Vector3.new(4, 1, 4) a.Position = Vector3.new(0, 10, 0) a.Parent = workspace \
             local b = Instance.new('Part') b.Name = 'B' b.Size = Vector3.new(1, 1, 1) b.Position = Vector3.new(0, 11, 0) b.Parent = workspace \
             local w = Instance.new('WeldConstraint') w.Part0 = a w.Part1 = b w.Parent = a \
             task.wait() \
             print('joints', #b:GetJoints(), #b:GetConnectedParts(), b.AssemblyRootPart == a, b:GetRootPart() == a) \
             print('touching', #a:GetTouchingParts()) \
             local m = Instance.new('Model') a.Parent = m b.Parent = m m.Parent = workspace m:BreakJoints() \
             print('broken', w.Parent, #b:GetJoints())",
            "",
        ),
        0.3,
    );
    assert!(has(&lines, S, "joints 1 2 true true"), "{lines:#?}");
    assert!(has(&lines, S, "broken nil 0"), "{lines:#?}");
}

#[test]
fn the_mouse_and_content_and_groups_answer() {
    let lines = run(
        place(
            "local p = game:GetService('Players'):WaitForChild('Player1') \
             print('rank', p:GetRankInGroup(1), p:GetRoleInGroup(1), p:IsInGroup(1)) \
             print('private', game.PrivateServerId == '', game.PrivateServerOwnerId) \
             local mps = game:GetService('MarketplaceService') \
             print('cast', pcall(mps.UserOwnsGamePassAsync, mps, 1, '123')) \
             local ids = {} game:GetService('ContentProvider'):PreloadAsync({'rbxassetid://1'}, function(id, status) table.insert(ids, status.Name) end) \
             print('preload', table.concat(ids, ','))",
            "local m = game:GetService('Players').LocalPlayer:GetMouse() \
             print('mouse', m.ClassName, typeof(m.Hit), typeof(m.Button1Down)) \
             debug.setmemorycategory('x') debug.resetmemorycategory()",
        ),
        0.5,
    );
    assert!(has(&lines, S, "rank 0 Guest false"), "{lines:#?}");
    assert!(has(&lines, S, "private true 0"), "{lines:#?}");
    assert!(has(&lines, S, "cast true false"), "{lines:#?}");
    assert!(has(&lines, S, "preload Success"), "{lines:#?}");
    assert!(
        has(&lines, C, "mouse PlayerMouse CFrame RBXScriptSignal"),
        "{lines:#?}"
    );
}

#[test]
fn a_proximity_prompt_triggers_on_both_sides() {
    let lines = run(
        place(
            "local player = game:GetService('Players'):WaitForChild('Player1') \
             local root = (player.Character or player.CharacterAdded:Wait()):WaitForChild('HumanoidRootPart') \
             local prompt = Instance.new('ProximityPrompt') prompt.RequiresLineOfSight = false prompt.Parent = Instance.new('Attachment', root) \
             prompt.Triggered:Connect(function(player) print('server triggered', player.Name) end) \
             game:GetService('ProximityPromptService').PromptTriggered:Connect(function(p, player) print('service', p == prompt, player.Name) end)",
            "local pps = game:GetService('ProximityPromptService') \
             pps.PromptShown:Connect(function(prompt) print('shown') prompt:InputHoldBegin() prompt:InputHoldEnd() end) \
             pps.PromptTriggered:Connect(function() print('client triggered') end)",
        ),
        2.0,
    );
    assert!(has(&lines, C, "shown"), "{lines:#?}");
    assert!(has(&lines, C, "client triggered"), "{lines:#?}");
    assert!(has(&lines, S, "server triggered Player1"), "{lines:#?}");
    assert!(has(&lines, S, "service true Player1"), "{lines:#?}");
}

#[test]
fn the_region3_queries_still_answer() {
    let lines = run(
        place(
            "local a = Instance.new('Part') a.Anchored = true a.Position = Vector3.new(0, 50, 0) a.Parent = workspace \
             local b = a:Clone() b.Position = Vector3.new(1, 50, 0) b.Parent = workspace \
             local r = Region3.new(Vector3.new(-3, 47, -3), Vector3.new(3, 53, 3)) \
             print('region', #workspace:FindPartsInRegion3(r), #workspace:FindPartsInRegion3(r, a), \
               #workspace:FindPartsInRegion3WithIgnoreList(r, {a, b}), #workspace:FindPartsInRegion3WithWhiteList(r, {b}), \
               #workspace:FindPartsInRegion3(r, nil, 1))",
            "",
        ),
        0.2,
    );
    assert!(has(&lines, S, "region 2 1 0 1 1"), "{lines:#?}");
}

#[test]
fn script_context_hears_script_errors() {
    let lines = run(
        place(
            "game:GetService('ScriptContext').Error:Connect(function(message, trace) \
               print('heard', message:find('boom') ~= nil, type(trace)) end) \
             task.spawn(function() error('boom') end)",
            "",
        ),
        0.3,
    );
    assert!(has(&lines, S, "heard true string"), "{lines:#?}");
}

#[test]
fn replicated_first_can_take_the_controls() {
    let mut dom = place("", "");
    let workspace = service(&dom, "Workspace");
    let folder = dom.new_instance("Folder", "Deep", Some(workspace));
    dom.new_instance("Part", "Probe", Some(folder));
    let first = service(&dom, "ReplicatedFirst");
    let local = dom.new_instance("LocalScript", "Disable", Some(first));
    let _ = dom.set_property(
        local,
        "Source",
        Variant::String(
            "workspace.DescendantAdded:Connect(function(d) if d.Name == 'Probe' then print('arrived', d:GetFullName()) end end) \
             local Player = game:GetService('Players').LocalPlayer \
             print('first', Player ~= nil) \
             local scripts = Player:WaitForChild('PlayerScripts') print('scripts') \
             local module = scripts:WaitForChild('PlayerModule') print('module') \
             local controls = require(module):GetControls() controls:Disable() print('disabled')"
                .into(),
        ),
    );
    dom.take_changes();
    let lines = run(dom, 0.5);
    assert!(has(&lines, C, "disabled"), "{lines:#?}");
    assert!(has(&lines, C, "arrived Workspace.Deep.Probe"), "{lines:#?}");
}

#[test]
fn player_gui_scripts_start_after_the_whole_place() {
    let mut dom = place("", "");
    let rs = service(&dom, "ReplicatedStorage");
    dom.new_instance("Folder", "Shared", Some(rs));
    let starter_gui = service(&dom, "StarterGui");
    let gui = dom.new_instance("ScreenGui", "Hud", Some(starter_gui));
    let local = dom.new_instance("LocalScript", "Hud", Some(gui));
    let _ = dom.set_property(
        local,
        "Source",
        Variant::String(
            "print('shared', game:GetService('ReplicatedStorage'):FindFirstChild('Shared') ~= nil, game:IsLoaded())"
                .into(),
        ),
    );
    dom.take_changes();
    let lines = run(dom, 0.3);
    assert!(has(&lines, C, "shared true true"), "{lines:#?}");
}
