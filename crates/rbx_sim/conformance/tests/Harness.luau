-- Shared by every conformance test, in real Roblox and in rbx_sim alike.
local RunService = game:GetService("RunService")

local Harness = {}

function Harness.beat()
	return RunService.Heartbeat:Wait()
end

function Harness.beats(n)
	for _ = 1, n do
		RunService.Heartbeat:Wait()
	end
end

-- A part in the test's own corner of the world. Unanchored ones stay with
-- the server, so a nearby player's client never simulates them.
function Harness.part(props)
	local part = Instance.new("Part")
	for key, value in props do
		part[key] = value
	end
	part.Parent = workspace
	if not part.Anchored then
		part:SetNetworkOwner(nil)
	end
	return part
end

function Harness.round(x, digits)
	local m = 10 ^ (digits or 4)
	return math.floor(x * m + 0.5) / m
end

return Harness
