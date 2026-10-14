local LogService = game:GetService("LogService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local codec = require(ReplicatedStorage:WaitForChild("__rbxnative_codec"))
local Live = require(ReplicatedStorage:WaitForChild("__rbxnative_live"))
local remote = ReplicatedStorage:WaitForChild("__rbxnative_remote")

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
	table.insert(buffer, {
		level = levels[messageType] or "output",
		message = message,
		time = os.clock() - t0,
	})
end)

local live = Live.new("client", function(message)
	table.insert(outgoing, message)
end, codec)

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
