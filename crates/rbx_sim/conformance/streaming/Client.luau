-- Streaming probe, the client's main half (a LocalScript in
-- StarterPlayerScripts): holds references to a few parts, waits on one
-- that is out of range, and answers the server's questions.
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local LogService = game:GetService("LogService")
local Players = game:GetService("Players")

local player = Players.LocalPlayer
local log = _G.streamLog
local function now()
	return workspace.DistributedGameTime
end

local result = { scriptStart = now(), scriptSnapshot = _G.streamSnapshot() }
local warnings = {}
-- (Local Run's VMs have no LogService.MessageOut: its harness reads the
-- client's Output instead.)
pcall(function()
	LogService.MessageOut:Connect(function(message, kind)
		if string.find(message, "Infinite yield", 1, true) then
			table.insert(warnings, { now(), message, kind.Name })
		end
	end)
end)

local probe = workspace:WaitForChild("S_Probe")

task.spawn(function()
	local t = now()
	local got = probe:WaitForChild("S_P3000", 3)
	result.waitTimeout = { got = got ~= nil, took = now() - t }
end)
task.spawn(function()
	local t = now()
	probe:WaitForChild("S_P3000")
	result.waitForever = { took = now() - t }
end)

local held = {}
for _, name in { "P16", "P128", "P512" } do
	task.spawn(function()
		local inst = probe:WaitForChild("S_" .. name)
		held[name] = inst
		inst.AncestryChanged:Connect(function(_, parent)
			table.insert(log, { now(), "ancestry", name, parent and parent.Name or "nil" })
		end)
		inst.Destroying:Connect(function()
			table.insert(log, { now(), "destroying", name })
		end)
		-- A change only this client made: does it survive streaming out?
		inst.Transparency = 0.5
	end)
end

local function refName(value)
	return value and value.Name or "nil"
end

ReplicatedStorage:WaitForChild("S_Call").OnClientInvoke = function(command)
	if command == "snapshot" then
		local model = workspace:FindFirstChild("S_DefaultM")
		return {
			at = now(),
			names = _G.streamSnapshot(),
			farRef = refName(ReplicatedStorage.S_FarRef.Value),
			nearRef = refName(ReplicatedStorage.S_NearRef.Value),
			primary = model and refName(model.PrimaryPart) or "no model",
			paused = player.GameplayPaused,
		}
	elseif command == "identity" then
		local out = {}
		for name, inst in held do
			local current = probe:FindFirstChild("S_" .. name)
			out[name] = {
				same = current == inst,
				parented = inst.Parent ~= nil,
				present = current ~= nil,
				transparency = current and current.Transparency or -1,
				heldTransparency = inst.Transparency,
			}
		end
		return out
	elseif command == "log" then
		return { log = log, first = _G.streamFirst, result = result, warnings = warnings }
	end
	return nil
end
ReplicatedStorage:WaitForChild("S_Ready"):FireServer()
