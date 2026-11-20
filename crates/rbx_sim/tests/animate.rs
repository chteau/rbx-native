//! The default `Animate` script on a played character: it runs on the
//! owner's client and plays the pose the humanoid is in.

use rbx_dom::{Variant, WeakDom};
use rbx_sim::{conformance, Input, Session};

/// A bare place with `source` as a LocalScript in StarterPlayerScripts.
fn place(source: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let starter_player = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "StarterPlayer"))
        .expect("StarterPlayer");
    let starter = dom.get(starter_player).unwrap().children()[0];
    let script = dom.new_instance("LocalScript", "Probe", Some(starter));
    let _ = dom.set_property(script, "Source", Variant::String(source.into()));
    dom.take_changes();
    dom
}

fn client_output(source: &str, seconds: f64) -> Vec<String> {
    let mut session = Session::new(place(source)).expect("the place runs");
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session.run_for(seconds);
    session.clients[0]
        .sim
        .take_output()
        .lines()
        .iter()
        .map(|l| l.to_string())
        .collect()
}

const PLAYING: &str = r#"
local function playing(a)
    local names = {}
    for _, t in a:GetPlayingAnimationTracks() do
        table.insert(names, t.Animation.Parent.Name .. "/" .. t.Name)
    end
    table.sort(names)
    return table.concat(names, " ")
end
local p = game:GetService("Players").LocalPlayer
local c = p.Character or p.CharacterAdded:Wait()
local h = c:WaitForChild("Humanoid")
local a = h:WaitForChild("Animator")
"#;

/// The character that spawned before the client arrived still animates:
/// its `Animate` starts with the player and idles. The two idles take
/// turns at random with a crossfade, so for a moment both can be playing.
#[test]
fn the_first_character_idles() {
    let output = client_output(
        &format!("{PLAYING}task.wait(1)\nprint('playing', playing(a))"),
        2.0,
    );
    assert!(
        output.iter().any(|l| matches!(
            l.as_str(),
            "playing idle/Animation1"
                | "playing idle/Animation2"
                | "playing idle/Animation1 idle/Animation2"
        )),
        "{output:#?}"
    );
}

fn key(session: &mut Session, code: &str, down: bool) {
    session.clients[0].sim.input(Input::Key {
        code: code.into(),
        down,
    });
}

fn playing_now(session: &mut Session) -> String {
    session.clients[0].sim.take_output();
    session.run_for(0.05);
    let out = session.clients[0].sim.take_output();
    out.lines()
        .iter()
        .find_map(|l| l.strip_prefix("playing ").map(str::to_owned))
        .unwrap_or_default()
}

/// Walking with W at 16 studs/s swaps idle for run; Space plays jump.
#[test]
fn walking_and_jumping_play_their_poses() {
    let mut session = Session::new(place(&format!(
        "{PLAYING}local seen = {{}}
game:GetService('RunService').Heartbeat:Connect(function()
    local now = playing(a)
    for name in string.gmatch(now, '%S+') do seen[name] = true end
    print('playing', now)
    if seen['jump/JumpAnim'] then print('jumped') end
end)"
    )))
    .expect("the place runs");
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session.run_for(1.0);
    key(&mut session, "W", true);
    session.run_for(0.6);
    let walking = playing_now(&mut session);
    assert!(walking.contains("run/RunAnim"), "{walking}");
    session.clients[0].sim.take_output();
    key(&mut session, "Space", true);
    session.run_for(0.2);
    let output = session.clients[0].sim.take_output();
    assert!(output.lines().iter().any(|l| l == "jumped"), "{output:?}");
}

/// `/e dance` in the chat bar plays the dance and is not a message.
#[test]
fn the_emote_command_dances() {
    let mut session = Session::new(place(&format!(
        "{PLAYING}game:GetService('RunService').Heartbeat:Connect(function()
    print('playing', playing(a))
end)"
    )))
    .expect("the place runs");
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session.run_for(1.0);
    session.clients[0].sim.take_chat();
    session.clients[0].sim.chat_say("/e dance");
    session.run_for(0.3);
    let dancing = playing_now(&mut session);
    assert!(dancing.contains("dance/"), "{dancing}");
    assert!(session.clients[0].sim.take_chat().is_empty());
}
