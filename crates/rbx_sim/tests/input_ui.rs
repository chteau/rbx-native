//! A client's touch gestures, gamepads, motion sensors and virtual input,
//! and the GUI, media and VR members that go with them.

use rbx_dom::{Variant, WeakDom};
use rbx_sim::{conformance, Input, Phase, Sensor, Session};

/// A bare place with a ScreenGui in StarterGui (ignoring the inset)
/// holding a 200x80 TextButton at (100, 100), and `source` as a
/// LocalScript in it.
fn place(source: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let starter = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "StarterGui"))
        .unwrap_or_else(|| dom.new_instance("StarterGui", "StarterGui", None));
    let screen = dom.new_instance("ScreenGui", "Screen", Some(starter));
    let _ = dom.set_property(screen, "IgnoreGuiInset", Variant::Bool(true));
    let udim2 = |x: i32, y: i32| {
        Variant::UDim2(rbx_dom::UDim2 {
            x: rbx_dom::UDim {
                scale: 0.0,
                offset: x,
            },
            y: rbx_dom::UDim {
                scale: 0.0,
                offset: y,
            },
        })
    };
    let button = dom.new_instance("TextButton", "Button", Some(screen));
    let _ = dom.set_property(button, "Position", udim2(100, 100));
    let _ = dom.set_property(button, "Size", udim2(200, 80));
    let script = dom.new_instance("LocalScript", "Probe", Some(screen));
    let _ = dom.set_property(script, "Source", Variant::String(source.into()));
    dom.take_changes();
    dom
}

fn playing(source: &str) -> Session {
    let mut session = Session::new(place(source)).expect("the place runs");
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session.run_for(1.0);
    session
}

fn send(session: &mut Session, inputs: Vec<Input>) {
    for input in inputs {
        session.input(0, input);
        session.step();
    }
}

fn touch(id: u32, x: f32, y: f32, phase: Phase) -> Input {
    Input::Touch {
        id,
        position: [x, y],
        phase,
    }
}

fn output(session: &mut Session) -> Vec<String> {
    session.clients[0].sim.take_output().lines().to_vec()
}

#[test]
fn touches_fire_their_events_and_gestures() {
    let mut session = playing(
        r#"
local UIS = game:GetService("UserInputService")
local button = script.Parent.Button
UIS.TouchStarted:Connect(function(t, gp) print("started", t.UserInputType.Name, t.Position.X, t.Position.Y, gp) end)
UIS.TouchEnded:Connect(function(t, gp) print("ended", gp) end)
UIS.TouchTap:Connect(function(ps, gp) print("tap", #ps, ps[1].X, gp) end)
UIS.TouchTapInWorld:Connect(function(p, ui) print("world", p.X, ui) end)
UIS.TouchPan:Connect(function(ps, total, v, state) print("pan", state.Name, total.X) end)
UIS.TouchSwipe:Connect(function(dir, n) print("swipe", dir.Name, n) end)
UIS.TouchLongPress:Connect(function(ps, state) print("long", state.Name) end)
button.TouchTap:Connect(function(ps) print("button tap", #ps) end)
button.Activated:Connect(function(input) print("activated", input.UserInputType.Name) end)
"#,
    );
    output(&mut session);
    // A tap in the world, then on the button.
    send(
        &mut session,
        vec![
            touch(0, 500.0, 400.0, Phase::Began),
            touch(0, 500.0, 400.0, Phase::Ended),
        ],
    );
    assert_eq!(
        output(&mut session),
        [
            "started Touch 500 342 false",
            "ended false",
            "tap 1 500 false",
            "world 500 false"
        ]
    );
    send(
        &mut session,
        vec![
            touch(1, 150.0, 120.0, Phase::Began),
            touch(1, 150.0, 120.0, Phase::Ended),
        ],
    );
    let lines = output(&mut session);
    assert!(lines.contains(&"activated Touch".to_owned()), "{lines:?}");
    assert!(lines.contains(&"button tap 1".to_owned()), "{lines:?}");
    assert!(lines.contains(&"world 150 true".to_owned()), "{lines:?}");
    // A fast drag pans and swipes.
    send(
        &mut session,
        vec![
            touch(2, 400.0, 400.0, Phase::Began),
            touch(2, 460.0, 400.0, Phase::Moved),
            touch(2, 520.0, 400.0, Phase::Moved),
            touch(2, 520.0, 400.0, Phase::Ended),
        ],
    );
    let lines = output(&mut session);
    let gestures: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|l| l.starts_with("pan") || l.starts_with("swipe"))
        .collect();
    assert_eq!(
        gestures,
        [
            "pan Begin 60",
            "pan Change 120",
            "pan End 120",
            "swipe Right 1"
        ]
    );
    // A held finger long-presses.
    session.input(0, touch(3, 400.0, 400.0, Phase::Began));
    session.run_for(0.7);
    send(&mut session, vec![touch(3, 400.0, 400.0, Phase::Ended)]);
    let lines = output(&mut session);
    assert!(lines.contains(&"long Begin".to_owned()), "{lines:?}");
    assert!(lines.contains(&"long End".to_owned()), "{lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("tap")), "{lines:?}");
}

#[test]
fn gamepads_connect_and_report_their_state() {
    let mut session = playing(
        r#"
local UIS = game:GetService("UserInputService")
UIS.GamepadConnected:Connect(function(pad) print("connected", pad.Name) end)
UIS.InputBegan:Connect(function(input)
    print("began", input.UserInputType.Name, input.KeyCode.Name)
    print("down", UIS:IsGamepadButtonDown(Enum.UserInputType.Gamepad1, Enum.KeyCode.ButtonA))
end)
UIS.InputChanged:Connect(function(input)
    if input.KeyCode == Enum.KeyCode.Thumbstick1 then
        print("stick", input.Position.X, input.Position.Y)
    end
end)
task.spawn(function()
UIS.GamepadConnected:Wait()
task.wait(0.1)
print("pads", #UIS:GetConnectedGamepads(), UIS:GetGamepadConnected(Enum.UserInputType.Gamepad1))
print("state", #UIS:GetGamepadState(Enum.UserInputType.Gamepad1), UIS:GamepadSupports(Enum.UserInputType.Gamepad1, Enum.KeyCode.ButtonX))
print("nav", UIS:IsNavigationGamepad(Enum.UserInputType.Gamepad1), #UIS:GetNavigationGamepads())
print("image", UIS:GetImageForKeyCode(Enum.KeyCode.ButtonA) ~= "", UIS:GetImageForKeyCode(Enum.KeyCode.Q))
end)
"#,
    );
    output(&mut session);
    send(
        &mut session,
        vec![
            Input::Gamepad {
                pad: 1,
                connected: true,
            },
            Input::GamepadButton {
                pad: 1,
                code: "ButtonA".into(),
                down: true,
            },
            Input::GamepadAxis {
                pad: 1,
                code: "Thumbstick1".into(),
                position: [0.5, -0.25, 0.0],
            },
        ],
    );
    session.run_for(0.3);
    assert_eq!(
        output(&mut session),
        [
            "connected Gamepad1",
            "began Gamepad1 ButtonA",
            "down true",
            "stick 0.5 -0.25",
            "pads 1 true",
            "state 18 true",
            "nav true 0",
            "image true "
        ]
    );
}

#[test]
fn sensors_virtual_input_and_modifiers() {
    let mut session = playing(
        r#"
local UIS = game:GetService("UserInputService")
print("gravity", UIS:GetDeviceGravity().Position, UIS:GetDeviceGravity().UserInputType.Name)
UIS.DeviceGravityChanged:Connect(function(g) print("gravity changed", g.Position.Y) end)
UIS.LastInputTypeChanged:Connect(function(t) print("last", t.Name) end)
UIS.PointerAction:Connect(function(wheel, pan, pinch, gp) print("pointer", wheel, pinch, gp) end)
UIS.InputBegan:Connect(function(input)
    if input.UserInputType == Enum.UserInputType.Keyboard then
        print("key", input.KeyCode.Name, input:IsModifierKeyDown(Enum.ModifierKey.Shift))
    end
end)
local virtual = UIS:CreateVirtualInput(game)
task.wait(0.2)
virtual:SendKey(true, Enum.KeyCode.E, false)
virtual:SendKey(false, Enum.KeyCode.E, false)
"#,
    );
    let first = output(&mut session);
    assert_eq!(
        first,
        [
            "gravity 0, 0, 0 Accelerometer",
            "last Keyboard",
            "key E false"
        ]
    );
    session.run_for(0.3);
    send(
        &mut session,
        vec![
            Input::Motion {
                sensor: Sensor::Gravity,
                vector: [0.0, -1.0, 0.0],
            },
            Input::Key {
                code: "LeftShift".into(),
                down: true,
            },
            Input::Key {
                code: "F".into(),
                down: true,
            },
            Input::Wheel { notches: 1.0 },
        ],
    );
    let lines = output(&mut session);
    assert_eq!(
        lines,
        [
            "last Accelerometer",
            "gravity changed -1",
            "last Keyboard",
            "key LeftShift true",
            "key F true",
            "last MouseWheel",
            "pointer 1 0 false"
        ]
    );
}

#[test]
fn buttons_selection_pages_and_hit_tests() {
    let mut session = playing(
        r#"
local screen = script.Parent
local button = screen.Button
button.SecondaryActivated:Connect(function(input) print("secondary", input.UserInputType.Name) end)
screen.SelectionChanged:Connect(function(me, old, new) print("screen selection", me, old, new and new.Name) end)
button.SelectionChanged:Connect(function(me) print("button selection", me) end)

local holder = Instance.new("Frame")
holder.Parent = screen
local layout = Instance.new("UIPageLayout")
for i = 1, 3 do
    local page = Instance.new("Frame")
    page.Name = "Page" .. i
    page.LayoutOrder = 4 - i
    page.Parent = holder
end
layout.Animated = false
layout.SortOrder = Enum.SortOrder.LayoutOrder
layout.Parent = holder
layout.PageEnter:Connect(function(p) print("enter", p.Name) end)
layout.PageLeave:Connect(function(p) print("leave", p.Name) end)
layout.Stopped:Connect(function(p) print("stopped", p.Name) end)
task.wait(0.1)
print("current", layout.CurrentPage.Name)
layout:Next()
task.wait()
layout:JumpToIndex(0)
task.wait()
layout:Previous()
print("still", layout.CurrentPage.Name)
layout.Circular = true
layout:Previous()
task.wait()
print("hits", #screen:GetGuiObjectsAtPosition(150, 120), screen:GetGuiObjectsAtPosition(150, 120)[1] == button)
print("player gui", #game:GetService("Players").LocalPlayer.PlayerGui:GetGuiObjectsAtPosition(5, 5))
game:GetService("GuiService").SelectedObject = button
"#,
    );
    session.run_for(0.5);
    send(
        &mut session,
        vec![
            Input::Move {
                position: [150.0, 120.0],
                delta: [0.0, 0.0],
            },
            Input::Button {
                button: 2,
                down: true,
            },
            Input::Button {
                button: 2,
                down: false,
            },
        ],
    );
    assert_eq!(
        output(&mut session),
        [
            "current Page3",
            "leave Page3",
            "enter Page2",
            "stopped Page2",
            "leave Page2",
            "enter Page3",
            "stopped Page3",
            "still Page3",
            "leave Page3",
            "enter Page1",
            "stopped Page1",
            "hits 1 true",
            "player gui 0",
            "button selection true",
            "screen selection false nil Button",
            "secondary MouseButton2"
        ]
    );
}

#[test]
fn tools_tell_context_action_service() {
    let mut session = playing(
        r#"
local CAS = game:GetService("ContextActionService")
CAS.LocalToolEquipped:Connect(function(tool) print("equipped", tool.Name, CAS:GetCurrentLocalToolIcon()) end)
CAS.LocalToolUnequipped:Connect(function(tool) print("unequipped", tool.Name) end)
local player = game:GetService("Players").LocalPlayer
local character = player.Character or player.CharacterAdded:Wait()
local tool = Instance.new("Tool")
tool.Name = "Sword"
tool.RequiresHandle = false
tool.TextureId = "rbxassetid://5"
tool.Parent = player.Backpack
task.wait(0.1)
character.Humanoid:EquipTool(tool)
task.wait(0.1)
character.Humanoid:UnequipTools()
"#,
    );
    session.run_for(0.5);
    assert_eq!(
        output(&mut session),
        ["equipped Sword rbxassetid://5", "unequipped Sword"]
    );
}

#[test]
fn media_vr_and_haptics_answer_as_a_desktop() {
    let mut session = playing(
        r#"
local VR = game:GetService("VRService")
local Haptic = game:GetService("HapticService")
local Sound = game:GetService("SoundService")
print("vr", VR:GetUserCFrame(Enum.UserCFrame.Head) == CFrame.identity, VR:GetUserCFrameEnabled(Enum.UserCFrame.Head))
print("pad", VR:GetTouchpadMode(Enum.VRTouchpad.Left).Name)
VR.TouchpadModeChanged:Connect(function(pad, mode) print("mode", pad.Name, mode.Name) end)
VR.NavigationRequested:Connect(function(cf, kind) print("navigate", cf.Position.X, kind.Name) end)
VR:SetTouchpadMode(Enum.VRTouchpad.Left, Enum.VRTouchpadMode.Touch)
VR:RequestNavigation(CFrame.new(3, 0, 0), Enum.UserCFrame.Head)
print("haptic", Haptic:IsVibrationSupported(Enum.UserInputType.Gamepad1), select('#', Haptic:GetMotor(Enum.UserInputType.Gamepad1, Enum.VibrationMotor.Large)))
print("listener", Sound:GetListener())
Sound:SetListener(Enum.ListenerType.CFrame, CFrame.new(1, 2, 3))
local kind, cf = Sound:GetListener()
print("listener", kind.Name, cf.Position)
local t0 = Sound:GetMixerTime()
task.wait(0.5)
print("mixer", Sound:GetMixerTime() - t0 > 0.4)
local camera = workspace.CurrentCamera
camera:ZoomToExtents(CFrame.new(0, 0, 0), Vector3.new(4, 4, 4))
print("zoom", math.round((camera.CFrame.Position - camera.Focus.Position).Magnitude * 10) / 10 > 0, camera.Focus.Position)
"#,
    );
    session.run_for(1.0);
    let lines = output(&mut session);
    assert_eq!(
        &lines[..7],
        [
            "vr true false",
            "pad VirtualThumbstick",
            "mode Left Touch",
            "navigate 3 Head",
            "haptic false 0",
            "listener Enum.ListenerType.Camera nil",
            "listener CFrame 1, 2, 3"
        ]
    );
    assert_eq!(lines[7..], ["mixer true", "zoom true 0, 0, 0"]);
}

#[test]
fn audio_and_video_players_keep_time() {
    let mut session = playing(
        r#"
local player = Instance.new("AudioPlayer")
player.Parent = workspace
local Sound = game:GetService("SoundService")
local id = player:Play(Sound:GetMixerTime() + 10)
print("scheduled", type(id), player:Cancel(id), player:Cancel(id))
player:Play(Sound:GetMixerTime() + 0.2)
task.wait(0.4)
print("playing", player.IsPlaying)

local tts = Instance.new("AudioTextToSpeech")
tts.Parent = workspace
print("tts pins", #tts:GetInputPins(), tts:GetOutputPins()[1], tts:LoadAsync().Name)
tts.Ended:Connect(function() print("tts ended") end)
tts:Play()
task.wait(0.1)

local recorder = Instance.new("AudioRecorder")
recorder.Parent = workspace
print("recorder", recorder:GetInputPins()[1], recorder:CanRecordAsync(), recorder:GetTemporaryContent().Uri ~= nil)
local gate = Instance.new("AudioGate")
print("gate", gate:GetInputPins()[1], gate:GetOutputPins()[1])
local input = Instance.new("AudioDeviceInput")
input:SetUserIdAccessList({ 1, 2 })
print("access", #input:GetUserIdAccessList())

local frame = Instance.new("VideoFrame")
frame.Parent = script.Parent
frame.Played:Connect(function(v) print("played", v) end)
frame.Paused:Connect(function(v) print("paused", v) end)
frame.Video = "rbxassetid://1"
frame:Play()
frame:Pause()
print("video", frame.IsLoaded, frame.TimeLength, frame.Playing)
local video = Instance.new("VideoPlayer")
print("video player", video:GetOutputPins()[1], video.IsPlaying)
"#,
    );
    session.run_for(1.0);
    assert_eq!(
        output(&mut session),
        [
            "scheduled number true false",
            "playing true",
            "tts pins 0 Output Failure",
            "tts ended",
            "recorder Input true true",
            "gate Input Output",
            "access 2",
            "played rbxassetid://1",
            "paused rbxassetid://1",
            "video false 0 false",
            "video player Output false"
        ]
    );
}

#[test]
fn a_touch_device_turns_the_mouse_into_a_finger() {
    let mut session = Session::new(place(
        r#"
local UIS = game:GetService("UserInputService")
UIS.TouchStarted:Connect(function(t) print("finger", t.Position.X) end)
UIS.TouchTap:Connect(function() print("tap") end)
UIS.InputBegan:Connect(function(i) print("began", i.UserInputType.Name) end)
"#,
    ))
    .expect("the place runs");
    let services = rbx_lua::server::Services {
        device: rbx_lua::server::Device {
            touch: true,
            keyboard: false,
            mouse: false,
            gamepad: false,
        },
        ..Default::default()
    };
    session.set_services(services);
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session.run_for(1.0);
    output(&mut session);
    send(
        &mut session,
        vec![
            Input::Move {
                position: [400.0, 300.0],
                delta: [0.0, 0.0],
            },
            Input::Button {
                button: 1,
                down: true,
            },
            Input::Button {
                button: 1,
                down: false,
            },
        ],
    );
    assert_eq!(output(&mut session), ["began Touch", "finger 400", "tap"]);
}

#[test]
fn chat_commands_bubbles_prompt_indicators_and_style_transitions() {
    let mut session = playing(
        r#"
local TextChatService = game:GetService("TextChatService")
local command = Instance.new("TextChatCommand")
command.PrimaryAlias = "/hi"
command.Parent = TextChatService
command.Triggered:Connect(function(source, text) print("triggered", text) end)
TextChatService.BubbleDisplayed:Connect(function(adornee, m) print("bubble", adornee.Name, m.Text) end)
local general = TextChatService:WaitForChild("TextChannels"):WaitForChild("RBXGeneral")
general:SendAsync("/HI there")
TextChatService:DisplayBubble(workspace, "hello")
local player = game:GetService("Players").LocalPlayer
print("groups", (pcall(TextChatService.GetChatGroupsAsync, TextChatService, { player })))
local props = TextChatService:FindFirstChildOfClass("ChatWindowConfiguration"):DeriveNewMessageProperties()
print("derived", props.ClassName)

local character = player.Character or player.CharacterAdded:Wait()
local root = character:WaitForChild("HumanoidRootPart")
local part = Instance.new("Part")
part.Anchored = true
part.Position = root.Position + Vector3.new(0, 0, 20)
local prompt = Instance.new("ProximityPrompt")
prompt.MaxActivationDistance = 5
prompt.MaxIndicatorDistance = 50
prompt.IndicatorShown:Connect(function() print("indicator shown") end)
prompt.Parent = part
part.Parent = workspace

local rule = Instance.new("StyleRule")
rule:SetPropertyTransition("Size", TweenInfo.new(0.5))
rule:SetDefaultPropertyTransition(TweenInfo.new(1))
print("transition", typeof(rule:GetPropertyTransitions().Size), rule:GetDefaultPropertyTransition().Time)
print("bad", pcall(rule.SetPropertyTransition, rule, "Size", 5))
local query = Instance.new("StyleQuery")
query:SetCondition("MinSize", Vector2.new(100, 0))
print("condition", query:GetCondition("MinSize").X, query:GetConditions().MinSize ~= nil)
"#,
    );
    session.run_for(1.0);
    let lines = output(&mut session);
    for line in [
        "triggered /HI there",
        "bubble Workspace hello",
        "groups false",
        "derived ChatWindowMessageProperties",
        "indicator shown",
        "transition TweenInfo 1",
        "condition 100 true",
    ] {
        assert!(lines.contains(&line.to_owned()), "{line}: {lines:?}");
    }
    assert!(
        lines.iter().any(|l| l.starts_with("bad false")),
        "{lines:?}"
    );
}
