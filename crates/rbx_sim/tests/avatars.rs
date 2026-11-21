//! `Players:GetHumanoidDescriptionFromUserId`, `Humanoid:ApplyDescription`
//! and `Players:CreateHumanoidModelFromDescription` through an app's avatars.

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_sim::{conformance, Avatars, Simulation};

/// One user, 7, whose description names a hat; a character dressed by it
/// wears the hat welded to its head, 1 stud above it.
struct OneUser;

impl Avatars for OneUser {
    fn description(&self, user_id: i64) -> Result<WeakDom, String> {
        if user_id != 7 {
            return Err(format!("no user {user_id}"));
        }
        let mut dom = WeakDom::new();
        let description = dom.new_instance("HumanoidDescription", "HumanoidDescription", None);
        let _ = dom.set_property(description, "HatAccessory", Variant::String("123".into()));
        Ok(dom)
    }

    fn character(&self, description: &WeakDom) -> Result<WeakDom, String> {
        let root = description.root_refs()[0];
        let hat = matches!(
            description.get(root).and_then(|d| d.properties().get("HatAccessory")),
            Some(Variant::String(s)) if s == "123"
        );
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Fresh", None);
        let head = part(&mut dom, "Head", model, 10.0);
        // A bigger head than the noob's: the body takes the description's
        // shape as well as its clothes.
        let _ = dom.set_property(
            head,
            "size",
            Variant::Vector3(Vector3Data {
                x: 4.0,
                y: 2.0,
                z: 2.0,
            }),
        );
        dom.new_instance("Humanoid", "Humanoid", Some(model));
        if hat {
            let accessory = dom.new_instance("Accessory", "Hat", Some(model));
            let handle = part(&mut dom, "Handle", accessory, 11.0);
            let weld = dom.new_instance("Weld", "AccessoryWeld", Some(handle));
            let _ = dom.set_property(weld, "Part0", Variant::Ref(handle));
            let _ = dom.set_property(weld, "Part1", Variant::Ref(head));
        }
        dom.new_instance("Shirt", "Shirt", Some(model));
        Ok(dom)
    }
}

fn part(dom: &mut WeakDom, name: &str, parent: Ref, y: f32) -> Ref {
    let part = dom.new_instance("Part", name, Some(parent));
    let _ = dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data { x: 0.0, y, z: 0.0 },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    part
}

#[test]
fn a_description_from_the_avatars_dresses_the_character() {
    let mut dom = conformance::bare_place();
    let service = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.name() == "ServerScriptService")
        })
        .unwrap();
    let script = dom.new_instance("Script", "Dress", Some(service));
    let _ = dom.set_property(
        script,
        "Source",
        Variant::String(
            r#"
local Players = game:GetService("Players")
local player = Players:GetPlayers()[1] or Players.PlayerAdded:Wait()
local character = player.Character or player.CharacterAdded:Wait()
local humanoid = character:WaitForChild("Humanoid")
local description = Players:GetHumanoidDescriptionFromUserId(7)
print("hat", description.HatAccessory)
print("missing", pcall(Players.GetHumanoidDescriptionFromUserId, Players, 8))
humanoid:ApplyDescription(description)
local hat = character:FindFirstChild("Hat")
local weld = hat and hat.Handle.AccessoryWeld
print("worn", hat ~= nil, character:FindFirstChildOfClass("Shirt") ~= nil)
print("welded", weld and weld.Part1 == character.Head)
print("above", hat and math.floor(hat.Handle.Position.Y - character.Head.Position.Y + 0.5))
print("applied", humanoid:GetAppliedDescription().HatAccessory)
print("head", character.Head.Size.X)
local built = Players:CreateHumanoidModelFromDescription(description, Enum.HumanoidRigType.R15)
print("built", built.Parent == nil, built:FindFirstChild("Hat") ~= nil, built.Humanoid:GetAppliedDescription().HatAccessory)
print("done")
"#
            .into(),
        ),
    );
    dom.take_changes();
    let mut sim = Simulation::new(dom).expect("the place runs");
    sim.set_avatars(std::rc::Rc::new(OneUser));
    sim.join("Player1", 1, None);
    sim.start();
    let mut lines = Vec::new();
    while sim.time() < 5.0 && !lines.iter().any(|l: &String| l == "done") {
        sim.step();
        lines.extend(sim.take_output().lines().iter().cloned());
    }
    let has = |want: &str| lines.iter().any(|l| l == want);
    assert!(has("hat 123"), "{lines:#?}");
    assert!(
        lines.iter().any(|l| l.starts_with("missing false")),
        "{lines:#?}"
    );
    assert!(has("worn true true"), "{lines:#?}");
    assert!(has("welded true"), "{lines:#?}");
    assert!(has("above 1"), "{lines:#?}");
    assert!(has("applied 123"), "{lines:#?}");
    assert!(has("head 4"), "{lines:#?}");
    assert!(has("built true true 123"), "{lines:#?}");
}

#[test]
fn a_client_dresses_its_own_dummy_but_not_a_replicated_humanoid() {
    let mut dom = conformance::bare_place();
    let starter = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "StarterPlayer"))
        .and_then(|s| dom.get(s).map(|i| i.children()[0]))
        .expect("StarterPlayerScripts");
    let script = dom.new_instance("LocalScript", "Lobby", Some(starter));
    let _ = dom.set_property(
        script,
        "Source",
        Variant::String(
            r#"
local Players = game:GetService("Players")
local character = Players.LocalPlayer.Character or Players.LocalPlayer.CharacterAdded:Wait()
local description = Players:GetHumanoidDescriptionFromUserId(7)
print("replicated", pcall(character:WaitForChild("Humanoid").ApplyDescription, character.Humanoid, description))
local dummy = Instance.new("Model")
Instance.new("Humanoid", dummy)
local head = Instance.new("Part", dummy)
head.Name = "Head"
dummy.Parent = workspace
dummy.Humanoid:ApplyDescription(description)
print("dummy", dummy:FindFirstChild("Hat") ~= nil)
"#
            .into(),
        ),
    );
    dom.take_changes();
    let mut session = rbx_sim::Session::new(dom).expect("the place runs");
    session.set_avatars(std::rc::Rc::new(OneUser));
    session.start();
    session.join("Player1", 1, None).expect("joins");
    let mut lines = Vec::new();
    for _ in 0..60 {
        session.step();
        for (_, output) in session.take_output() {
            lines.extend(output.lines().iter().cloned());
        }
    }
    assert!(
        lines.iter().any(|l| l
            == "replicated false Humanoid::ApplyDescription() can only be called by the backend server"),
        "{lines:#?}"
    );
    assert!(lines.iter().any(|l| l == "dummy true"), "{lines:#?}");
}
