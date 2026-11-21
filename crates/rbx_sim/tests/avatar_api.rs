//! The character, avatar and animation API a server script sees, member by
//! member: curves and their keys, keyframe trees, rig descriptions, node
//! pins, Humanoid accessories and body parts, HumanoidDescription emotes
//! and accessories, bans. Each expectation is what the AnimationApi and
//! AvatarApi goldens recorded on a live server.

use rbx_dom::{Variant, WeakDom};
use rbx_sim::{conformance, Session};

fn place(server: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "ServerScriptService"))
        .unwrap_or_else(|| dom.new_instance("ServerScriptService", "ServerScriptService", None));
    let script = dom.new_instance("Script", "Server", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(server.into()));
    dom.take_changes();
    dom
}

/// What the server printed after `seconds` of a run with no player.
fn run(server: &str, seconds: f64) -> Vec<String> {
    let mut session = Session::new(place(server)).expect("the place runs");
    session.start();
    session.run_for(seconds);
    session
        .take_output()
        .into_iter()
        .flat_map(|(_, output)| output.lines().to_vec())
        .collect()
}

fn lines(server: &str) -> Vec<String> {
    run(server, 0.2)
}

#[test]
fn float_curve_keys_insert_index_and_evaluate() {
    let out = lines(
        r#"
local c = Instance.new("FloatCurve")
print(c:GetValueAtTime(0) == nil, c.Length)
local r = c:InsertKey(FloatCurveKey.new(0, 0, Enum.KeyInterpolationMode.Linear))
print(r[1], r[2])
r = c:InsertKey(FloatCurveKey.new(4, 8, Enum.KeyInterpolationMode.Linear))
print(r[1], r[2])
r = c:InsertKey(FloatCurveKey.new(4, 4, Enum.KeyInterpolationMode.Linear))
print(r[1], r[2], c.Length)
print(c:GetValueAtTime(2), c:GetValueAtTime(-1), c:GetValueAtTime(9))
local i = c:GetKeyIndicesAtTime(1)
print(i[1], i[2])
print(c:GetKeyAtIndex(2).Value, pcall(c.GetKeyAtIndex, c, 3))
print(c:RemoveKeyAtIndex(1, 5), c.Length)
print(pcall(c.InsertKey, c, 3))
"#,
    );
    assert_eq!(
        out,
        [
            "true 0",
            "true 0",
            "true 1",
            "false 1 2",
            "2 0 4",
            "1 2",
            "4 false FloatCurve index is out of range",
            "2 0",
            "false Unable to cast double to FloatCurveKey",
        ]
    );
}

#[test]
fn cubic_keys_with_unset_tangents_are_flat() {
    let out = lines(
        r#"
local c = Instance.new("FloatCurve")
print(c:SetKeys({ FloatCurveKey.new(5, 5), FloatCurveKey.new(0, 0), FloatCurveKey.new(5, 5) }))
print(c:GetValueAtTime(2.5))
local k = FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Linear)
print(tostring(k), k.LeftTangent, typeof(k))
print(pcall(function() k.RightTangent = 1 end))
print(k == FloatCurveKey.new(1, 2, Enum.KeyInterpolationMode.Linear))
print(c:Clone().Length)
"#,
    );
    assert_eq!(
        out,
        [
            "2",
            "2.5",
            "{Linear, 1, 2, nil, nil} nil FloatCurveKey",
            "false Attempting to set a right tangent value on a curve key for which the interpolation mode is not Cubic",
            "true",
            "2",
        ]
    );
}

#[test]
fn marker_rotation_value_and_vector_curves() {
    let out = lines(
        r#"
local m = Instance.new("MarkerCurve")
local r = m:InsertMarkerAtTime(1, "hit")
print(r[1], r[2], m:GetMarkerAtIndex(1).Value, m:GetMarkers()[1].Time, m.Length)
print(pcall(m.GetMarkerAtIndex, m, 2))
local rot = Instance.new("RotationCurve")
rot:InsertKey(RotationCurveKey.new(0, CFrame.new(), Enum.KeyInterpolationMode.Linear))
rot:InsertKey(RotationCurveKey.new(1, CFrame.Angles(0, math.pi / 2, 0), Enum.KeyInterpolationMode.Linear))
local _, y = rot:GetValueAtTime(0.5):ToOrientation()
print(string.format("%.4f", y))
local v = Instance.new("ValueCurve")
print(#v:GetKeys(), v:GetValueAtTime(0))
local v3 = Instance.new("Vector3Curve")
v3:X():InsertKey(FloatCurveKey.new(0, 3))
local at = v3:GetValueAtTime(0)
print(at[1], at[2], v3:X().ClassName)
local e = Instance.new("EulerRotationCurve")
e:Y():InsertKey(FloatCurveKey.new(0, math.pi / 2))
local _, ey = e:GetRotationAtTime(0):ToOrientation()
print(string.format("%.4f", ey))
"#,
    );
    assert_eq!(
        out,
        [
            "true 0 hit 1 1",
            "false MarkerCurve index is out of range",
            "0.7854",
            "0 nil",
            "3 nil FloatCurve",
            "1.5708",
        ]
    );
}

#[test]
fn keyframe_trees_ignore_the_wrong_class() {
    let out = lines(
        r#"
local ks = Instance.new("KeyframeSequence")
local kf = Instance.new("Keyframe")
ks:AddKeyframe(kf)
ks:AddKeyframe(Instance.new("Folder"))
ks:AddKeyframe(nil)
print(#ks:GetKeyframes(), #ks:GetChildren())
kf:AddPose(Instance.new("Pose"))
kf:AddPose(Instance.new("NumberPose"))
kf:AddMarker(Instance.new("KeyframeMarker"))
print(#kf:GetPoses(), #kf:GetMarkers(), #kf:GetChildren())
ks:RemoveKeyframe(Instance.new("Keyframe"))
ks:RemoveKeyframe(kf)
print(#ks:GetKeyframes(), kf.Parent)
"#,
    );
    assert_eq!(out, ["1 1", "1 1 3", "0 nil"]);
}

#[test]
fn providers_refuse_registration_on_a_server() {
    let out = lines(
        r#"
local ksp = game:GetService("KeyframeSequenceProvider")
print(pcall(ksp.RegisterActiveKeyframeSequence, ksp, Instance.new("KeyframeSequence")))
local acp = game:GetService("AnimationClipProvider")
print(pcall(acp.RegisterAnimationClip, acp, Instance.new("KeyframeSequence")))
local a = Instance.new("Animator")
print(pcall(a.StepAnimations, a, 0))
print(pcall(a.RegisterEvaluationParallelCallback, a, function() end))
print(a:ApplyJointVelocities({}) == nil)
print(a:GetTrackByAnimationId("rbxassetid://1"))
"#,
    );
    assert_eq!(
        out,
        [
            "false Usage error: RegisterActiveKeyframeSequence can only be used in Solo mode.",
            "false Usage error: RegisterAnimationClip can only be called in Studio.",
            "false The current thread cannot call 'StepAnimations' (lacking capability PluginOrOpenCloud)",
            "false RegisterEvaluationParallelCallback is not enabled!",
            "true",
            "nil",
        ]
    );
}

#[test]
fn rig_descriptions_and_node_pins() {
    let out = lines(
        r#"
local rig = Instance.new("HumanoidRigDescription")
print(#rig:GetR15JointNames(), #rig:GetR6JointNames(), #rig:GetJointNames())
print(rig:GetJointFromName("Head"))
local d = Instance.new("DigitsRigDescription")
d:SetFingerTip(1, Vector3.new(1, 2, 3))
print(d:GetFingerTip(1))
local n = Instance.new("AnimationNodeDefinition")
local fired = 0
n.InputPinsChanged:Connect(function() fired += 1 end)
n:AddInputPin("a")
n:AddInputPin("a")
n:RemoveInputPin("zz")
n:SetOrderedInputPinNames({ "b", "c" })
task.wait()
print(table.concat(n:GetOrderedInputPinNames(), ","), fired)
"#,
    );
    assert_eq!(out, ["15 6 22", "nil", "0, 0, 0", "b,c 4"]);
}

#[test]
fn humanoid_description_emotes_and_accessories() {
    let out = lines(
        r#"
local d = Instance.new("HumanoidDescription")
local events = 0
d.EmotesChanged:Connect(function() events += 1 end)
d:AddEmote("Wave", 507770818)
d:AddEmote("Wave", 507770239)
print(table.concat(d:GetEmotes().Wave, ","))
print(pcall(d.AddEmote, d, "Wave", "x"))
d:SetEquippedEmotes({ "Wave" })
print(d:GetEquippedEmotes()[1].Name, d:GetEquippedEmotes()[1].Slot)
d.HatAccessory = "1,2"
d.BackAccessory = "3"
local list = d:GetAccessories(true)
print(#list, list[1].AssetId, list[2].AssetId, list[3].AccessoryType)
print(pcall(function() return d.EmotesDataInternal end))
task.wait()
print(events)
"#,
    );
    assert_eq!(
        out,
        [
            "507770239,507770818",
            "false Unable to cast string to int64",
            "Wave 1",
            "3 2 1 Enum.AccessoryType.Back",
            "false EmotesDataInternal is not a valid member of HumanoidDescription \"HumanoidDescription\"",
            "2",
        ]
    );
}

#[test]
fn bans_validate_like_a_live_server() {
    let out = lines(
        r#"
local Players = game:GetService("Players")
print(pcall(Players.BanAsync, Players, {}))
print(pcall(Players.BanAsync, Players, { UserIds = { 1 }, Duration = 0 }))
print(pcall(Players.BanAsync, Players, "x"))
print(pcall(Players.UnbanAsync, Players, {}))
print(pcall(Players.GetBanHistoryAsync, Players, "x"))
print(Players:GetBanHistoryAsync(1).IsFinished)
"#,
    );
    assert_eq!(
        out,
        [
            "false Config must contain field UserIds with type array",
            "false Duration must be a positive number or -1",
            "false Unable to cast string to Dictionary",
            "false Config must contain field UserIds with type array",
            "false Unable to cast string to User",
            "true",
        ]
    );
}

#[test]
fn accessories_and_body_parts_on_a_rig() {
    let out = run(
        r#"
local rig = Instance.new("Model")
rig.Name = "Rig"
for _, name in { "HumanoidRootPart", "Head", "LeftUpperArm", "LeftHand", "RightFoot" } do
	local p = Instance.new("Part")
	p.Name = name
	p.Anchored = true
	p.Size = Vector3.new(1, 1, 1)
	p.Parent = rig
end
local h = Instance.new("Humanoid")
h.RigType = Enum.HumanoidRigType.R15
h.Parent = rig
rig.Parent = workspace
print(pcall(h.AddAccessory, h, Instance.new("Part")))
local hat = Instance.new("Accessory")
local handle = Instance.new("Part")
handle.Name = "Handle"
handle.Parent = hat
h:AddAccessory(hat)
local w = rig.Head:FindFirstChild("HeadWeld")
print(hat.Parent == rig, w and w.Part1 == handle)
print(#h:GetAccessories())
h:RemoveAccessories()
print(#h:GetAccessories(), hat.Parent)
print(h:GetBodyPartR15(rig.LeftUpperArm), h:GetLimb(rig.RightFoot))
print(pcall(h.GetLimb, h, nil))
local new = Instance.new("Part")
new.Name = "NewHand"
print(h:ReplaceBodyPartR15(Enum.BodyPartR15.LeftHand, new), new.Parent == rig, rig:FindFirstChild("LeftHand"))
print(h:GetMoveVelocity())
"#,
        1.0,
    );
    assert_eq!(
        out,
        [
            "false AddAccessory should be passed a valid Accessory object.",
            "true true",
            "1",
            "0 nil",
            "Enum.BodyPartR15.LeftUpperArm Enum.Limb.RightLeg",
            "false GetLimb must be passed a non-nil part.",
            "true true nil",
            "0, 0, 0",
        ]
    );
}
