//! What the editor's core GUI reads and asks of a run: leaderstats in the
//! player list, the menu's Reset (and `ResetButtonCallback`), the chat bar,
//! and `ParticleEmitter:Emit`/`Clear` for the renderer.

use rbx_dom::{Variant, WeakDom};
use rbx_sim::{conformance, Burst, Peer, Session};

fn place(server: &str, client: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let service = |dom: &mut WeakDom, class: &str| {
        dom.root_refs()
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
            .unwrap_or_else(|| dom.new_instance(class, class, None))
    };
    let sss = service(&mut dom, "ServerScriptService");
    let script = dom.new_instance("Script", "Server", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(server.into()));
    let starter = service(&mut dom, "StarterGui");
    let local = dom.new_instance("LocalScript", "Client", Some(starter));
    let _ = dom.set_property(local, "Source", Variant::String(client.into()));
    dom.take_changes();
    dom
}

fn playing(server: &str, client: &str) -> Session {
    let mut session = Session::new(place(server, client)).expect("the place runs");
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session.run_for(1.0);
    session
}

const LEADERSTATS: &str = r#"
game:GetService("Players").PlayerAdded:Connect(function(player)
	local stats = Instance.new("Folder")
	stats.Name = "leaderstats"
	local coins = Instance.new("IntValue")
	coins.Name = "Coins"
	coins.Value = 42
	coins.Parent = stats
	local rank = Instance.new("StringValue")
	rank.Name = "Rank"
	rank.Value = "Gold"
	rank.Parent = stats
	stats.Parent = player
end)
"#;

#[test]
fn leaderstats_are_player_list_columns() {
    let mut session = playing(LEADERSTATS, "");
    let core = session.sim_mut(Peer::Client(0)).unwrap().core_gui();
    assert_eq!(core.stats, ["Coins", "Rank"]);
    let players = core.players.expect("the player list is on");
    assert_eq!(players[0].name, "Player1");
    assert_eq!(players[0].stats, ["42", "Gold"]);
    assert!(core.can_reset);
}

#[test]
fn set_core_gui_enabled_toggles_each_part() {
    let mut session = playing(LEADERSTATS, "");
    let core = session.sim_mut(Peer::Client(0)).unwrap().core_gui();
    assert!(core.chat && core.chat_bar && core.topbar);
    assert!(core.health.is_some() && core.hotbar.is_some() && core.players.is_some());
    assert_eq!(core.players.unwrap()[0].display_name, "Player1");

    let client = r#"
local StarterGui = game:GetService("StarterGui")
StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.PlayerList, false)
StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.Chat, false)
StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.Health, false)
"#;
    let mut session = playing(LEADERSTATS, client);
    let core = session.sim_mut(Peer::Client(0)).unwrap().core_gui();
    assert!(core.players.is_none() && core.health.is_none() && !core.chat);
    assert!(core.hotbar.is_some() && core.topbar);

    let client = r#"
local StarterGui = game:GetService("StarterGui")
StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.All, false)
StarterGui:SetCoreGuiEnabled(Enum.CoreGuiType.Backpack, true)
StarterGui:SetCore("ChatBarDisabled", true)
"#;
    let mut session = playing("", client);
    let core = session.sim_mut(Peer::Client(0)).unwrap().core_gui();
    assert!(core.hotbar.is_some() && core.players.is_none() && !core.chat && !core.chat_bar);
}

#[test]
fn topbar_enabled_false_hides_the_core_gui() {
    let client = r#"game:GetService("StarterGui"):SetCore("TopbarEnabled", false)"#;
    let mut session = playing(LEADERSTATS, client);
    let core = session.sim_mut(Peer::Client(0)).unwrap().core_gui();
    assert!(!core.topbar && !core.chat);
    assert!(core.players.is_none() && core.health.is_none() && core.hotbar.is_none());
}

fn health(session: &Session) -> f64 {
    let sim = session.sim(Peer::Server).unwrap();
    let (_, character) = sim.local_player().unwrap();
    let dom = sim.dom();
    let humanoid = dom
        .get(character.unwrap())
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|h| dom.get(*h).is_some_and(|i| i.class() == "Humanoid"))
        .unwrap();
    match dom.get(humanoid).unwrap().properties().get("Health") {
        Some(Variant::Float32(v)) => f64::from(*v),
        Some(Variant::Float64(v)) => *v,
        _ => 100.0,
    }
}

#[test]
fn reset_kills_the_character_on_the_server() {
    let mut session = playing("", "");
    assert!(health(&session) > 0.0);
    session.sim_mut(Peer::Client(0)).unwrap().reset_character();
    session.run_for(0.1);
    assert_eq!(health(&session), 0.0);
}

#[test]
fn reset_button_callback_false_turns_reset_off() {
    let client = r#"game:GetService("StarterGui"):SetCore("ResetButtonCallback", false)"#;
    let mut session = playing("", client);
    let sim = session.sim_mut(Peer::Client(0)).unwrap();
    assert!(!sim.core_gui().can_reset);
    sim.reset_character();
    session.run_for(0.1);
    assert!(health(&session) > 0.0);
}

#[test]
fn reset_button_callback_event_fires_instead() {
    let client = r#"
local event = Instance.new("BindableEvent")
event.Event:Connect(function() print("reset asked") end)
game:GetService("StarterGui"):SetCore("ResetButtonCallback", event)
"#;
    let mut session = playing("", client);
    session.take_output();
    session.sim_mut(Peer::Client(0)).unwrap().reset_character();
    session.run_for(0.1);
    let printed: Vec<String> = session
        .take_output()
        .into_iter()
        .map(|(_, o)| format!("{o:?}"))
        .collect();
    assert!(
        printed.iter().any(|l| l.contains("reset asked")),
        "{printed:?}"
    );
    assert!(health(&session) > 0.0);
}

#[test]
fn the_chat_bar_sends_to_everyone_in_general() {
    let mut session = playing("", "");
    let sim = session.sim_mut(Peer::Client(0)).unwrap();
    sim.take_chat();
    sim.chat_say("hello there");
    session.run_for(0.2);
    let lines = session.sim_mut(Peer::Client(0)).unwrap().take_chat();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].text, "hello there");
    assert_eq!(lines[0].prefix, "Player1:");
    assert_eq!(lines[0].channel, "RBXGeneral");
    assert_eq!(lines[0].user_id, 1);
}

#[test]
fn emit_and_clear_reach_the_renderer() {
    let server = r#"
local part = Instance.new("Part")
part.Anchored = true
part.Parent = workspace
local emitter = Instance.new("ParticleEmitter")
emitter.Enabled = false
emitter.Parent = part
task.wait(0.2)
emitter:Emit(30)
emitter:Emit()
emitter:Clear()
"#;
    let mut session = playing(server, "");
    let bursts: Vec<Burst> = session
        .sim_mut(Peer::Server)
        .unwrap()
        .take_bursts()
        .into_iter()
        .map(|(_, b)| b)
        .collect();
    assert_eq!(bursts, [Burst::Emit(30), Burst::Emit(16), Burst::Clear]);
}

const IDLE: &str = r#"
local t = Instance.new("Tool")
t.Name = "late"
t.RequiresHandle = false
t.Parent = game:GetService("StarterPack")
"#;

const ICONS_AND_LABELS: &str = r#"
local idle = script.Parent.Idle
idle.Enabled = true
local t = Instance.new("Tool")
t.Name = if script.Enabled and not idle.Disabled then "enabled" else "disabled"
t.TextureId = "rbxassetid://123"
t.RequiresHandle = false
t.Parent = game:GetService("StarterPack")
local rig = Instance.new("Model")
rig.Name = "Rig"
local head = Instance.new("Part")
head.Name = "Head"
head.Anchored = true
head.Size = Vector3.new(1, 1, 1)
head.Position = Vector3.new(0, 10, 0)
head.Parent = rig
local humanoid = Instance.new("Humanoid")
humanoid.Parent = rig
rig.Parent = workspace
"#;

#[test]
fn tool_icons_rig_labels_and_script_enabled() {
    let mut dom = place(ICONS_AND_LABELS, "");
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ServerScriptService")
        })
        .unwrap();
    let idle = dom.new_instance("Script", "Idle", Some(sss));
    let _ = dom.set_property(idle, "Source", Variant::String(IDLE.into()));
    let _ = dom.set_property(idle, "Disabled", Variant::Bool(true));
    dom.take_changes();
    let mut session = Session::new(dom).expect("the place runs");
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session.run_for(1.0);
    let core = session.sim_mut(Peer::Client(0)).unwrap().core_gui();
    let hotbar = core.hotbar.expect("the backpack is on");
    let names: Vec<&str> = hotbar.iter().map(|s| s.name.as_str()).collect();
    let out: Vec<String> = session
        .take_output()
        .into_iter()
        .flat_map(|(_, o)| o.lines().to_vec())
        .collect();
    assert!(
        names.contains(&"enabled") && names.contains(&"late"),
        "{names:?} {out:?}"
    );
    let icon = hotbar.iter().find(|s| s.name == "enabled").unwrap();
    assert_eq!(icon.icon, "rbxassetid://123");
    let rig = core
        .labels
        .iter()
        .find(|l| l.text == "Rig")
        .expect("the rig's label");
    assert_eq!(rig.at, [0.0, 11.5, 0.0]);
    assert!(
        !core.labels.iter().any(|l| l.text == "Player1"),
        "no label over yourself"
    );
}
