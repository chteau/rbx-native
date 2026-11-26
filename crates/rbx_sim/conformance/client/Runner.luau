-- The client half of the suite (a LocalScript in StarterPlayerScripts):
-- the server's tests call `Call:InvokeClient(player, name, ...)` and the
-- module `name` in ReplicatedStorage.ConformanceClient answers.
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

_G.ps_loaded_at_start = game:IsLoaded()
local folder = ReplicatedStorage:WaitForChild("ConformanceClient")
local shared = require(folder:WaitForChild("Shared"))

local ch = { shared = shared }
function ch.beat()
	return RunService.Heartbeat:Wait()
end
function ch.beats(n)
	for _ = 1, n do
		RunService.Heartbeat:Wait()
	end
end

folder:WaitForChild("Call").OnClientInvoke = function(name, ...)
	local ok, result = pcall(require(folder:WaitForChild(name)), ch, ...)
	if not ok then
		return { error = tostring(result) }
	end
	return result
end
folder:WaitForChild("Ready"):FireServer()
