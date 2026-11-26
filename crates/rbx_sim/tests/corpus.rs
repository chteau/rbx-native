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

#[test]
fn a_camera_tween_completes() {
    let lines = run(
        place(
            "",
            "local camera = workspace:WaitForChild('Camera') \
             local blur = Instance.new('BlurEffect', game:GetService('Lighting')) blur.Size = 0 \
             local ts = game:GetService('TweenService') \
             local info = TweenInfo.new(0.3) \
             local a = ts:Create(blur, info, { Size = 24 }) \
             local b = ts:Create(camera, info, { FieldOfView = 60 }) \
             a:Play() b:Play() \
             b.Completed:Wait() print('camera done', math.floor(camera.FieldOfView + 0.5)) \
             print('blur', blur.Size)",
        ),
        1.0,
    );
    assert!(has(&lines, C, "camera done 60"), "{lines:#?}");
    assert!(has(&lines, C, "blur 24"), "{lines:#?}");
}

#[test]
fn a_remote_drops_what_cannot_cross() {
    let lines = run(
        place(
            "local rf = Instance.new('RemoteFunction') rf.Name = 'Roll' rf.Parent = game:GetService('ReplicatedStorage') \
             local hidden = Instance.new('Folder', game:GetService('ServerStorage')) \
             rf.OnServerInvoke = function() return { Rarity = { Label = 'Rare' }, Item = { Name = 'Taco', Gear = hidden, Script = function() end } } end",
            "local rf = game:GetService('ReplicatedStorage'):WaitForChild('Roll') \
             local ok, result = pcall(rf.InvokeServer, rf) \
             print('roll', ok, ok and result.Rarity.Label, ok and result.Item.Name, ok and result.Item.Gear, ok and result.Item.Script)",
        ),
        0.5,
    );
    assert!(has(&lines, C, "roll true Rare Taco nil nil"), "{lines:#?}");
}

#[test]
fn the_legacy_gui_tweens_run() {
    let lines = run(
        place(
            "",
            "local f = Instance.new('Frame') \
             f:TweenPosition(UDim2.fromOffset(100, 0), 'Out', 'Linear', 0.2, false, function(s) print('done', s.Name, f.Position.X.Offset) end) \
             print('busy', f:TweenSize(UDim2.fromOffset(5, 5))) \
             print('override', f:TweenSizeAndPosition(UDim2.fromOffset(5, 5), UDim2.fromOffset(100, 0), Enum.EasingDirection.In, Enum.EasingStyle.Quad, 0.3, true))",
        ),
        1.0,
    );
    assert!(has(&lines, C, "busy false"), "{lines:#?}");
    assert!(has(&lines, C, "override true"), "{lines:#?}");
    assert!(has(&lines, C, "done Canceled 0"), "{lines:#?}");
}

#[test]
fn the_server_can_turn_on_a_player_s_screen_gui() {
    let mut dom = place(
        "game:GetService('Players').PlayerAdded:Connect(function(player) \
         player.PlayerGui:WaitForChild('RulesUI').Enabled = true end)",
        "local gui = game:GetService('Players').LocalPlayer:WaitForChild('PlayerGui'):WaitForChild('RulesUI') \
         task.wait(0.5) print('rules', gui.Enabled)",
    );
    let starter = service(&dom, "StarterGui");
    let rules = dom.new_instance("ScreenGui", "RulesUI", Some(starter));
    let _ = dom.set_property(rules, "Enabled", Variant::Bool(false));
    dom.take_changes();
    let lines = run(dom, 1.0);
    assert!(has(&lines, C, "rules true"), "{lines:#?}");
}

#[test]
fn a_humanoid_s_root_part_is_its_humanoid_root_part() {
    let lines = run(
        place(
            "",
            "local player = game:GetService('Players').LocalPlayer local character = player.Character or player.CharacterAdded:Wait() \
             local humanoid = character:WaitForChild('Humanoid') \
             print('root', humanoid.RootPart == character:FindFirstChild('HumanoidRootPart')) \
             character.HumanoidRootPart.Name = 'Gone' \
             print('renamed', humanoid.RootPart)",
        ),
        0.5,
    );
    assert!(has(&lines, C, "root true"), "{lines:#?}");
    assert!(has(&lines, C, "renamed nil"), "{lines:#?}");
}

#[test]
fn a_ragdoll_in_the_physics_state_lives_without_its_neck() {
    let lines = run(
        place(
            "",
            "local player = game:GetService('Players').LocalPlayer local character = player.Character or player.CharacterAdded:Wait() \
             local humanoid = character:WaitForChild('Humanoid') \
             task.wait(0.2) \
             humanoid:ChangeState(Enum.HumanoidStateType.Physics) \
             local folder = Instance.new('Folder', game:GetService('ReplicatedStorage')) \
             for _, d in character:GetDescendants() do if d:IsA('Motor6D') then d.Parent = folder end end \
             task.wait(0.3) print('ragdoll', humanoid.Health, humanoid:GetState().Name)",
        ),
        0.8,
    );
    assert!(has(&lines, C, "ragdoll 100 Physics"), "{lines:#?}");
}

#[test]
fn the_legacy_body_movers_act_on_their_part() {
    let lines = run(
        place(
            "local function part(x) local p = Instance.new('Part') p.Size = Vector3.new(2, 2, 2) p.Position = Vector3.new(x, 20, 0) p.Parent = workspace return p end \
             local floated = part(0) local f = Instance.new('BodyForce') f.Force = Vector3.new(0, floated.AssemblyMass * workspace.Gravity, 0) f.Parent = floated \
             local pushed = part(10) local v = Instance.new('BodyVelocity') v.Velocity = Vector3.new(10, 0, 0) v.MaxForce = Vector3.one * math.huge v.Parent = pushed \
             local held = part(20) held.Position = Vector3.new(20, 10, 0) local bp = Instance.new('BodyPosition') bp.Position = Vector3.new(20, 20, 0) bp.MaxForce = Vector3.one * math.huge bp.Parent = held \
             local turned = part(30) local g = Instance.new('BodyGyro') g.CFrame = CFrame.Angles(0, math.rad(90), 0) g.MaxTorque = Vector3.one * math.huge g.Parent = turned \
             local lifted = part(40) local onlyY = Instance.new('BodyPosition') onlyY.Position = Vector3.new(99, 25, 99) onlyY.MaxForce = Vector3.new(0, math.huge, 0) onlyY.Parent = lifted \
             task.wait(3) \
             print('floated', floated.Position.Y > 18.5, math.round(floated.AssemblyLinearVelocity.Y)) \
             print('pushed', math.round(pushed.AssemblyLinearVelocity.X), math.round(pushed.AssemblyLinearVelocity.Y)) \
             print('held', math.round(held.Position.Y)) \
             print('turned', math.round(turned.Orientation.Y)) \
             print('lifted', math.round(lifted.Position.X), math.round(lifted.Position.Y))",
            "",
        ),
        3.5,
    );
    // A force steps like a VectorForce: the first free kick drops it a
    // little before the force has caught up, then it hangs still.
    assert!(has(&lines, S, "floated true 0"), "{lines:#?}");
    assert!(has(&lines, S, "pushed 10 0"), "{lines:#?}");
    assert!(has(&lines, S, "held 20"), "{lines:#?}");
    assert!(has(&lines, S, "turned 90"), "{lines:#?}");
    assert!(has(&lines, S, "lifted 40 25"), "{lines:#?}");
}

#[test]
fn a_body_position_holds_a_character_up() {
    let lines = run(
        place(
            "",
            "local character = game:GetService('Players').LocalPlayer.Character or game:GetService('Players').LocalPlayer.CharacterAdded:Wait() \
             local root = character:WaitForChild('HumanoidRootPart') task.wait(0.5) \
             local bp = Instance.new('BodyPosition') bp.MaxForce = Vector3.new(0, math.huge, 0) bp.Position = root.Position + Vector3.new(0, 10, 0) bp.Parent = root \
             task.wait(2) print('up', math.abs(root.Position.Y - bp.Position.Y) < 0.5)",
        ),
        3.0,
    );
    assert!(has(&lines, C, "up true"), "{lines:#?}");
}

#[test]
fn queries_follow_parts_that_come_go_and_move() {
    let lines = run(
        place(
            "local function hit(x) local r = workspace:Raycast(Vector3.new(x, 100, 0), Vector3.new(0, -200, 0)) return r and math.round(r.Position.Y) end \
             local p = Instance.new('Part') p.Anchored = true p.Size = Vector3.new(4, 2, 4) p.Position = Vector3.new(50, 10, 0) p.Parent = workspace \
             print('placed', hit(50)) \
             p.Position = Vector3.new(60, 20, 0) print('moved', hit(50), hit(60)) \
             p.Parent = game:GetService('ReplicatedStorage') print('stored', hit(60)) \
             p.Parent = workspace print('back', hit(60)) \
             local f = Instance.new('Part') f.Size = Vector3.new(4, 2, 4) f.Position = Vector3.new(80, 50, 0) f.Parent = workspace \
             task.wait(2) print('fell', hit(80) < 10) \
             p:Destroy() print('gone', hit(60))",
            "",
        ),
        2.5,
    );
    for line in [
        "placed 11",
        "moved 0 21",
        "stored 0",
        "back 21",
        "fell true",
        "gone 0",
    ] {
        assert!(has(&lines, S, line), "{line}: {lines:#?}");
    }
}

#[test]
fn model_move_to_carries_the_model_by_its_primary_part() {
    let lines = run(
        place(
            "",
            "local m = Instance.new('Model') \
             local a = Instance.new('Part') a.Anchored = true a.Position = Vector3.new(0, 5, 0) a.Parent = m \
             local b = Instance.new('Part') b.Anchored = true b.Position = Vector3.new(4, 5, 0) b.Parent = m \
             m.PrimaryPart = a m.Parent = workspace \
             m:MoveTo(Vector3.new(10, 20, 30)) \
             print('moved', a.Position, b.Position)",
        ),
        1.0,
    );
    assert!(has(&lines, C, "moved 10, 20, 30 14, 20, 30"), "{lines:#?}");
}

#[test]
fn model_move_to_rises_out_of_what_it_would_overlap() {
    let lines = run(
        place(
            "",
            "local block = Instance.new('Part') block.Anchored = true block.Size = Vector3.new(4, 4, 4) block.Position = Vector3.new(40, 2, 0) block.Parent = workspace \
             local m = Instance.new('Model') \
             local a = Instance.new('Part') a.Anchored = true a.Size = Vector3.new(2, 2, 2) a.Parent = m \
             m.PrimaryPart = a m.Parent = workspace \
             m:MoveTo(Vector3.new(40, 2, 0)) \
             print('lifted', a.Position.Y)",
        ),
        1.0,
    );
    assert!(has(&lines, C, "lifted 5"), "{lines:#?}");
}

#[test]
fn a_replicated_first_loader_sees_its_character_arrive() {
    let mut dom = place("", "");
    let first = service(&dom, "ReplicatedFirst");
    let module = dom.new_instance(
        "ModuleScript",
        "Loader",
        Some(service(&dom, "ReplicatedStorage")),
    );
    let _ = dom.set_property(module, "Source", Variant::String("return 1".into()));
    let local = dom.new_instance("LocalScript", "Boot", Some(first));
    let _ = dom.set_property(
        local,
        "Source",
        Variant::String(
            "require(game:GetService('ReplicatedStorage'):WaitForChild('Loader')) \
             local character = game:GetService('Players').LocalPlayer.CharacterAdded:Wait() \
             print('arrived', character.Parent == workspace)"
                .into(),
        ),
    );
    dom.take_changes();
    let lines = run(dom, 2.0);
    assert!(has(&lines, C, "arrived true"), "{lines:#?}");
}

#[test]
fn a_destroyed_instance_still_reads_as_it_was() {
    let lines = run(
        place(
            "local p = Instance.new('Part') p.Name = 'X' p.Transparency = 0.5 p.Parent = workspace \
             local v = Instance.new('IntValue') v.Value = 7 v.Parent = p \
             p:Destroy() \
             print('kept', v.Value, p.Transparency, p.Name, p.Parent, v.Parent)",
            "",
        ),
        0.5,
    );
    assert!(has(&lines, S, "kept 7 0.5 X nil nil"), "{lines:#?}");
}
