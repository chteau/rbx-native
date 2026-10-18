//! Adds a LocalScript to a place that logs every touch event it gets, on
//! screen and in the Output: open the result in rbxstudio, pick a phone in
//! the Device menu and press Play to try the touch stand-in
//! (`rbx_play::touch`).
//!
//! ```text
//! cargo run -p rbx_play --example touch_demo -- <in.rbxl> <out.rbxl>
//! ```

use rbx_dom::{Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

const DEMO: &str = r#"local UIS = game:GetService("UserInputService")
local Players = game:GetService("Players")
local gui = Instance.new("ScreenGui")
gui.ResetOnSpawn = false
local label = Instance.new("TextLabel")
label.Size = UDim2.new(0.6, 0, 0, 150)
label.Position = UDim2.new(0.2, 0, 0, 0)
label.BackgroundColor3 = Color3.new(0, 0, 0)
label.BackgroundTransparency = 0.25
label.TextColor3 = Color3.new(1, 1, 1)
label.Font = Enum.Font.Code
label.TextSize = 13
label.TextXAlignment = Enum.TextXAlignment.Left
label.TextYAlignment = Enum.TextYAlignment.Top
label.Parent = gui
gui.Parent = Players.LocalPlayer:WaitForChild("PlayerGui")
local lines = {}
local function log(s)
	print("[touchdemo] " .. s)
	table.insert(lines, 1, s)
	if #lines > 10 then table.remove(lines) end
	label.Text = table.concat(lines, "\n")
end
local function p(v) return string.format("%d,%d", v.X, v.Y) end
log(string.format("Touch=%s Mouse=%s Keyboard=%s Last=%s", tostring(UIS.TouchEnabled), tostring(UIS.MouseEnabled), tostring(UIS.KeyboardEnabled), UIS:GetLastInputType().Name))
local moved = 0
UIS.InputBegan:Connect(function(i, gp) log("InputBegan " .. i.UserInputType.Name .. " gp=" .. tostring(gp)) end)
UIS.TouchStarted:Connect(function(t) moved = 0 log("TouchStarted " .. p(t.Position)) end)
UIS.TouchMoved:Connect(function() moved += 1 end)
UIS.TouchEnded:Connect(function(t) log("TouchEnded " .. p(t.Position) .. " moves=" .. moved) end)
UIS.TouchTap:Connect(function(ps) log("TouchTap " .. p(ps[1])) end)
UIS.TouchLongPress:Connect(function(ps, s) log("TouchLongPress " .. s.Name) end)
UIS.TouchPan:Connect(function(ps, tr, v, s) if s ~= Enum.UserInputState.Change then log("TouchPan " .. s.Name .. " " .. p(tr)) end end)
UIS.TouchSwipe:Connect(function(d, n) log("TouchSwipe " .. d.Name .. " x" .. n) end)
UIS.TouchPinch:Connect(function(ps, sc, v, s) log(string.format("TouchPinch %s scale=%.2f fingers=%d", s.Name, sc, #ps)) end)
UIS.TouchRotate:Connect(function(ps, r, v, s) log(string.format("TouchRotate %s %.0f deg", s.Name, r)) end)

-- The other ways in: the service by index, a button's and a frame's own events.
local IndexUIS = game.UserInputService
log("game.UserInputService: Mouse=" .. tostring(IndexUIS.MouseEnabled))
local button = Instance.new("TextButton")
button.Name = "DemoButton"
button.Size = UDim2.new(0, 120, 0, 50)
button.Position = UDim2.new(1, -330, 1, -70)
button.Text = "Button"
button.Parent = gui
button.InputBegan:Connect(function(input)
	log("Button.InputBegan " .. input.UserInputType.Name .. " typeof=" .. typeof(input))
	if input.UserInputType == Enum.UserInputType.Touch then
		input.Changed:Connect(function(property)
			if property == "UserInputState" and input.UserInputState == Enum.UserInputState.End then
				log("Button touch Changed -> End")
			end
		end)
	end
end)
button.Activated:Connect(function(input) log("Button.Activated " .. input.UserInputType.Name) end)
button.TouchTap:Connect(function() log("Button.TouchTap") end)
local pad = Instance.new("Frame")
pad.Name = "DemoPad"
pad.Size = UDim2.new(0, 160, 0, 90)
pad.Position = UDim2.new(1, -170, 1, -230)
pad.BackgroundColor3 = Color3.fromRGB(40, 90, 160)
pad.Active = true
pad.Parent = gui
pad.TouchPan:Connect(function(ps, tr, v, s) if s ~= Enum.UserInputState.Change then log("Pad.TouchPan " .. s.Name .. " " .. p(tr)) end end)
pad.TouchPinch:Connect(function(ps, sc, v, s) if s == Enum.UserInputState.End then log(string.format("Pad.TouchPinch End %.2f", sc)) end end)
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: touch_demo <in.rbxl> <out.rbxl>");
        std::process::exit(2);
    };
    let bytes = std::fs::read(input).unwrap();
    let mut dom: WeakDom = rbx_binary::deserialize(&bytes).unwrap();
    let root = dom.root_refs()[0];
    let kids = if dom.get(root).unwrap().class() == "DataModel" {
        dom.get(root).unwrap().children().to_vec()
    } else {
        dom.root_refs().to_vec()
    };
    let starter = kids
        .into_iter()
        .find(|r| dom.get(*r).unwrap().class() == "StarterPlayer")
        .unwrap();
    let sps = dom
        .get(starter)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|r| dom.get(*r).unwrap().class() == "StarterPlayerScripts")
        .unwrap_or_else(|| {
            dom.new_instance(
                "StarterPlayerScripts",
                "StarterPlayerScripts",
                Some(starter),
            )
        });
    let s = dom.new_instance("LocalScript", "TouchDemo", Some(sps));
    dom.set_property(s, "Source", Variant::String(DEMO.into()))
        .unwrap();
    let db = ReflectionDatabase::shared();
    let out =
        rbx_binary::serialize_with_defaults(&dom, |c, k| db.stored_default(c, k).cloned()).unwrap();
    std::fs::write(output, out).unwrap();
}
