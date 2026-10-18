local LogService = game:GetService("LogService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local codec = require(ReplicatedStorage:WaitForChild("__rbxnative_codec"))
local Live = require(ReplicatedStorage:WaitForChild("__rbxnative_live"))
local Debug = require(ReplicatedStorage:WaitForChild("__rbxnative_debug"))
local Print = require(ReplicatedStorage:WaitForChild("__rbxnative_print"))
-- Game scripts' MarketplaceService is rewritten to this module's proxy
-- (test purchases); start it before any of them prompts.
require(ReplicatedStorage:WaitForChild("__rbxnative_market")).start()
local remote = ReplicatedStorage:WaitForChild("__rbxnative_remote")
-- Touch controls for a simulated touch device (see the module).
task.spawn(function()
	local ok, err = pcall(function()
		require(ReplicatedStorage:WaitForChild("__rbxnative_touch")).start()
		require(ReplicatedStorage:WaitForChild("__rbxnative_touch_input")).start()
	end)
	if not ok then
		warn("[rbx-native] touch controls: " .. tostring(err))
	end
end)

local t0 = os.clock()
local buffer = {}
local outgoing = {}

local levels = {
	[Enum.MessageType.MessageOutput] = "output",
	[Enum.MessageType.MessageInfo] = "info",
	[Enum.MessageType.MessageWarning] = "warning",
	[Enum.MessageType.MessageError] = "error",
}

LogService.MessageOut:Connect(function(message, messageType)
	local entry = {
		level = levels[messageType] or "output",
		message = message,
		time = os.clock() - t0,
	}
	if entry.level == "output" or entry.level == "warning" then
		local record = Print.take(message)
		if record then
			entry.parts = record.parts
			entry.origin = record.origin
		end
	end
	table.insert(buffer, entry)
end)

local live = Live.new("client", function(message)
	table.insert(outgoing, message)
end, codec)

-- A paused thread waits on the editor: its messages go ahead of the tree
-- snapshot's chunks in the one-per-tick relay.
Debug.attach("client", function(m)
	table.insert(outgoing, 1, m)
end)

remote.OnClientEvent:Connect(function(payload)
	if type(payload) ~= "table" or payload.kind ~= "cmd" or type(payload.command) ~= "table" then
		return
	end
	local cmd = payload.command
	local kind = cmd.type
	if kind == "watch" then
		live.watch(cmd.items or {})
	elseif kind == "set" then
		live.set(cmd.target, cmd.prop, cmd.value)
	elseif kind == "set_attribute" then
		live.set_attribute(cmd.target, cmd.name, cmd.value)
	elseif kind == "set_tags" then
		live.set_tags(cmd.target, cmd.tags or {})
	elseif kind == "destroy" then
		live.destroy(cmd.target)
	elseif kind == "reparent" then
		live.reparent(cmd.target, cmd.parent)
	elseif kind == "create" then
		live.create(cmd.class, cmd.name, cmd.parent)
	elseif kind == "resync" then
		live.resync()
	elseif kind == "debug_set" or kind == "debug_resume" or kind == "debug_watch" then
		Debug.command(cmd)
	elseif kind == "pointer" then
		local ok, err = pcall(function()
			require(ReplicatedStorage:WaitForChild("__rbxnative_touch_input")).setPointerInside(cmd.inside == true)
		end)
		if not ok then
			warn("[rbx-native] pointer: " .. tostring(err))
		end
	end
end)

task.spawn(live.snapshot)

local Stats = game:GetService("Stats")
local RunService = game:GetService("RunService")
local Players = game:GetService("Players")
local frames = 0
RunService.RenderStepped:Connect(function()
	frames += 1
end)

task.spawn(function()
	while true do
		local started = os.clock()
		frames = 0
		task.wait(1)
		-- task.wait resumes a little late; rate over the real interval.
		local m = { fps = frames / math.max(os.clock() - started, 0.001) }
		local function read(key, f)
			local okRead, value = pcall(f)
			if okRead and type(value) == "number" then
				m[key] = value
			end
		end
		read("memory_mb", function()
			return Stats:GetTotalMemoryUsageMb()
		end)
		read("instances", function()
			return Stats.InstanceCount
		end)
		read("send_kbps", function()
			return Stats.DataSendKbps
		end)
		read("receive_kbps", function()
			return Stats.DataReceiveKbps
		end)
		read("frame_ms", function()
			return Stats.FrameTime * 1000
		end)
		read("ping_ms", function()
			return Players.LocalPlayer:GetNetworkPing() * 1000
		end)
		table.insert(outgoing, { type = "stats", side = "client", metrics = m })
	end
end)

local UserInputService = game:GetService("UserInputService")
local GuiService = game:GetService("GuiService")

local textFocus = false
local lastDeviceKey = nil

-- TextBoxFocused never fires for the CoreGui chat bar: TextChatService
-- reports its focus itself. Polled with the rest of the report.
local function chatFocused()
	local okRead, focused = pcall(function()
		return game:GetService("TextChatService").ChatInputBarConfiguration.IsFocused
	end)
	return okRead and focused == true
end

local function readDevice()
	local width, height = 0, 0
	pcall(function()
		local camera = workspace.CurrentCamera
		if camera then
			width = camera.ViewportSize.X
			height = camera.ViewportSize.Y
		end
	end)
	local function flag(f)
		local okRead, value = pcall(f)
		return okRead and value == true
	end
	local m = {
		type = "device",
		viewport_width = width,
		viewport_height = height,
		touch_enabled = flag(function()
			return UserInputService.TouchEnabled
		end),
		keyboard_enabled = flag(function()
			return UserInputService.KeyboardEnabled
		end),
		gamepad_enabled = flag(function()
			return UserInputService.GamepadEnabled
		end),
		ten_foot = flag(function()
			return GuiService:IsTenFootInterface()
		end),
		on_screen_keyboard_visible = flag(function()
			return UserInputService.OnScreenKeyboardVisible
		end),
		text_focus = textFocus or chatFocused(),
	}
	local okPref, pref = pcall(function()
		return UserInputService.PreferredInput.Name
	end)
	if okPref and type(pref) == "string" then
		m.preferred_input = pref
	end
	local okLast, last = pcall(function()
		return UserInputService:GetLastInputType().Name
	end)
	if okLast and type(last) == "string" then
		m.last_input = last
	end
	return m
end

local function reportDevice()
	local m = readDevice()
	local key = table.concat({
		m.viewport_width,
		m.viewport_height,
		tostring(m.touch_enabled),
		tostring(m.keyboard_enabled),
		tostring(m.gamepad_enabled),
		tostring(m.ten_foot),
		m.preferred_input or "",
		tostring(m.on_screen_keyboard_visible),
		tostring(m.text_focus),
		m.last_input or "",
	}, "|")
	if key ~= lastDeviceKey then
		lastDeviceKey = key
		table.insert(outgoing, m)
	end
end

pcall(function()
	UserInputService.TextBoxFocused:Connect(function()
		textFocus = true
		reportDevice()
	end)
	UserInputService.TextBoxFocusReleased:Connect(function()
		textFocus = false
		reportDevice()
	end)
end)

-- Polled: ViewportSize/CurrentCamera/PreferredInput change signals vary by client.
task.spawn(function()
	while true do
		pcall(reportDevice)
		task.wait(0.25)
	end
end)

-- At most one FireServer per tick: 20 calls/s.
local FIRE_PERIOD = 0.05

while true do
	task.wait(FIRE_PERIOD)
	if #buffer > 0 then
		local batch = buffer
		buffer = {}
		remote:FireServer(batch)
	elseif #outgoing > 0 then
		local message = table.remove(outgoing, 1)
		remote:FireServer({ kind = "msg", message = message })
	end
end
