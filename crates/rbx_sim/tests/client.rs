//! Play (local)'s client half: input through `ContextActionService` and
//! `UserInputService`, our default controls and camera.

use rbx_dom::{Variant, WeakDom};
use rbx_sim::{conformance, Input, MouseBehavior, Session, Simulation};

fn key(code: &str, down: bool) -> Input {
    Input::Key {
        code: code.into(),
        down,
    }
}

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
    if !source.is_empty() {
        let script = dom.new_instance("LocalScript", "Probe", Some(starter));
        let _ = dom.set_property(script, "Source", Variant::String(source.into()));
    }
    dom.take_changes();
    dom
}

/// A session with one local player, seen from their client: steps run
/// the whole session, everything else is the client's.
struct Play(Session);

impl Play {
    fn new(dom: WeakDom) -> Play {
        let mut session = Session::new(dom).expect("the place runs");
        session.start();
        session.join("Player1", 1, None).expect("joins");
        Play(session)
    }

    fn step(&mut self) {
        self.0.step();
    }

    fn run_for(&mut self, seconds: f64) {
        self.0.run_for(seconds);
    }
}

impl std::ops::Deref for Play {
    type Target = Simulation;
    fn deref(&self) -> &Simulation {
        &self.0.clients[0].sim
    }
}

impl std::ops::DerefMut for Play {
    fn deref_mut(&mut self) -> &mut Simulation {
        &mut self.0.clients[0].sim
    }
}

fn playing(source: &str) -> Play {
    let mut sim = Play::new(place(source));
    // The character spawns and settles.
    sim.run_for(1.0);
    sim
}

fn root(sim: &Play) -> [f32; 3] {
    let (_, character) = sim.local_player().expect("joined");
    let dom = sim.dom();
    let model = dom.get(character.expect("a character")).unwrap();
    let root = model
        .children()
        .iter()
        .find(|c| dom.get(**c).is_some_and(|i| i.name() == "HumanoidRootPart"))
        .copied()
        .unwrap();
    match dom.get(root).unwrap().properties().get("CFrame") {
        Some(Variant::CFrame(c)) => [c.position.x, c.position.y, c.position.z],
        _ => panic!("no root CFrame"),
    }
}

fn lines(sim: &mut Play) -> Vec<String> {
    sim.take_output().lines().to_vec()
}

#[test]
fn w_walks_away_from_the_camera() {
    let mut sim = playing("");
    let start = root(&sim);
    let (camera, fov) = sim.camera().expect("a camera");
    assert_eq!(fov, 70.0);
    // Behind the character (it faces -Z) and above it.
    assert!(camera.position.z > start[2] + 8.0, "{camera:?} {start:?}");
    assert!(camera.position.y > start[1], "{camera:?} {start:?}");
    sim.input(key("W", true));
    sim.run_for(1.0);
    sim.input(key("W", false));
    sim.step();
    let moved = root(&sim);
    assert!(moved[2] < start[2] - 10.0, "{start:?} -> {moved:?}");
    let output = lines(&mut sim);
    assert!(!output.iter().any(|l| l.contains("rror")), "{output:#?}");
}

#[test]
fn a_higher_binding_sinks_movement() {
    let mut sim = playing(
        r#"
local CAS = game:GetService("ContextActionService")
local UIS = game:GetService("UserInputService")
CAS:BindActionAtPriority("block", function(name, state, input)
    print("action", name, state.Name, input.KeyCode.Name)
    return Enum.ContextActionResult.Sink
end, false, Enum.ContextActionPriority.High.Value, Enum.KeyCode.W)
UIS.InputBegan:Connect(function(input, processed)
    print("began", input.KeyCode.Name, processed, UIS:IsKeyDown(Enum.KeyCode.W))
end)
print("bound", CAS:GetBoundActionInfo("block").priorityLevel)
"#,
    );
    let start = root(&sim);
    sim.input(key("W", true));
    sim.run_for(0.5);
    let moved = root(&sim);
    assert!((moved[2] - start[2]).abs() < 0.5, "{start:?} -> {moved:?}");
    let output = lines(&mut sim);
    let has = |want: &str| output.iter().any(|l| l == want);
    assert!(has("bound 3000"), "{output:#?}");
    assert!(has("action block Begin W"), "{output:#?}");
    assert!(has("began W true true"), "{output:#?}");
}

#[test]
fn passing_lets_movement_through() {
    let mut sim = playing(
        r#"
local CAS = game:GetService("ContextActionService")
CAS:BindActionAtPriority("look", function()
    return Enum.ContextActionResult.Pass
end, false, Enum.ContextActionPriority.High.Value, Enum.KeyCode.W)
"#,
    );
    let start = root(&sim);
    sim.input(key("W", true));
    sim.run_for(0.5);
    assert!(root(&sim)[2] < start[2] - 3.0);
}

#[test]
fn shift_locks_the_mouse_and_first_person_hides_the_body() {
    let mut sim = playing(
        r#"
local UIS = game:GetService("UserInputService")
UIS.InputBegan:Connect(function(input, processed)
    print("began", input.KeyCode.Name, processed, game.Players.LocalPlayer.DevEnableMouseLock)
end)
"#,
    );
    assert_eq!(sim.mouse_behavior(), MouseBehavior::Default);
    sim.input(key("LeftShift", true));
    sim.input(key("LeftShift", false));
    sim.step();
    let output = lines(&mut sim);
    assert_eq!(
        sim.mouse_behavior(),
        MouseBehavior::LockCenter,
        "{output:#?}"
    );
    sim.input(key("LeftShift", true));
    sim.input(key("LeftShift", false));
    sim.step();
    assert_eq!(sim.mouse_behavior(), MouseBehavior::Default);
    // Right-drag turns the camera, holding the cursor where it is.
    let (before, _) = sim.camera().unwrap();
    sim.input(Input::Button {
        button: 2,
        down: true,
    });
    sim.step();
    sim.input(Input::Move {
        position: [400.0, 300.0],
        delta: [50.0, 0.0],
    });
    sim.step();
    assert_eq!(sim.mouse_behavior(), MouseBehavior::LockCurrentPosition);
    let (after, _) = sim.camera().unwrap();
    assert_ne!(before.rotation, after.rotation);
    sim.input(Input::Button {
        button: 2,
        down: false,
    });
    // Zoomed all the way in: first person, the mouse locked and the
    // character see-through.
    for _ in 0..40 {
        sim.input(Input::Wheel { notches: 1.0 });
    }
    sim.step();
    sim.step();
    assert_eq!(sim.mouse_behavior(), MouseBehavior::LockCenter);
    let (camera, _) = sim.camera().unwrap();
    let at = root(&sim);
    let height = camera.position.y - at[1];
    assert!((height - 1.5).abs() < 0.2, "{camera:?} {at:?}");
    let (_, character) = sim.local_player().unwrap();
    let dom = sim.dom();
    let head = dom
        .get(character.unwrap())
        .unwrap()
        .children()
        .iter()
        .find(|c| dom.get(**c).is_some_and(|i| i.name() == "Head"))
        .copied()
        .unwrap();
    assert_eq!(
        dom.get(head)
            .unwrap()
            .properties()
            .get("LocalTransparencyModifier"),
        Some(&Variant::Float32(1.0))
    );
}

#[test]
fn a_wall_behind_pulls_the_camera_in() {
    let mut dom = place("");
    let workspace = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
        .unwrap();
    let wall = dom.new_instance("Part", "Wall", Some(workspace));
    let _ = dom.set_property(wall, "Anchored", Variant::Bool(true));
    let _ = dom.set_property(
        wall,
        "Size",
        Variant::Vector3(rbx_dom::Vector3Data {
            x: 40.0,
            y: 40.0,
            z: 1.0,
        }),
    );
    let _ = dom.set_property(
        wall,
        "CFrame",
        Variant::CFrame(rbx_dom::CFrameData {
            position: rbx_dom::Vector3Data {
                x: 0.0,
                y: 10.0,
                z: 5.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    dom.take_changes();
    let mut sim = Play::new(dom);
    sim.run_for(1.0);

    let (camera, _) = sim.camera().unwrap();
    assert!(camera.position.z < 4.6, "{camera:?}");
}

fn distance(camera: &rbx_dom::CFrameData, at: [f32; 3]) -> f32 {
    let d = [
        camera.position.x - at[0],
        camera.position.y - at[1],
        camera.position.z - at[2],
    ];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

#[test]
fn after_a_respawn_the_camera_follows_the_new_character_upright() {
    let mut dom = place("");
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "ServerScriptService"))
        .expect("ServerScriptService");
    let reset = dom.new_instance("Script", "Reset", Some(sss));
    let _ = dom.set_property(
        reset,
        "Source",
        Variant::String(
            "local player = game:GetService('Players').PlayerAdded:Wait()\n\
             local character = player.Character or player.CharacterAdded:Wait()\n\
             task.wait(0.5)\n\
             character:PivotTo(CFrame.new(40, 5, 0))\n\
             task.wait(0.5)\n\
             character:FindFirstChildOfClass('Humanoid').Health = 0\n"
                .into(),
        ),
    );
    dom.take_changes();
    let mut sim = Play::new(dom);
    sim.run_for(1.0);
    let (before, _) = sim.camera().unwrap();
    let start = distance(&before, root(&sim));
    // Dead at 1 s, a new character RespawnTime (5 s) later, settled after.
    sim.run_for(8.0);
    let respawned = root(&sim);
    let (after, _) = sim.camera().unwrap();
    assert!(after.rotation[3].abs() < 1e-3, "rolled: {after:?}");
    let now = distance(&after, respawned);
    assert!(
        (now - start).abs() < 1.0,
        "{now} studs from the new character, {start} before: {after:?} {respawned:?}"
    );
    // Looking at the new character.
    let look = [-after.rotation[2], -after.rotation[5], -after.rotation[8]];
    let along: f32 = (0..3)
        .map(|i| look[i] * (respawned[i] - [after.position.x, after.position.y, after.position.z][i]))
        .sum();
    assert!(along > 0.9 * now, "{after:?} {respawned:?}");
    let output = lines(&mut sim);
    assert!(!output.iter().any(|l| l.contains("rror")), "{output:#?}");
}
