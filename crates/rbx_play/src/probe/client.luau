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
