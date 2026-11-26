-- Streaming probe, the client's earliest half (a LocalScript in
-- ReplicatedFirst): logs every probe instance (named "S_...") that arrives
-- or leaves the client, from before the place has loaded, in server time.
local Players = game:GetService("Players")

local log = {}
_G.streamLog = log

local function now()
	return workspace.DistributedGameTime
end
local function isProbe(inst)
	return string.sub(inst.Name, 1, 2) == "S_"
end
local function snapshot()
	local names = {}
	for _, d in workspace:GetDescendants() do
		if isProbe(d) then
			table.insert(names, string.sub(d.Name, 3))
		end
	end
	table.sort(names)
	return names
end
_G.streamSnapshot = snapshot

local first = { start = snapshot(), startLoaded = game:IsLoaded(), startAt = now() }
_G.streamFirst = first

workspace.DescendantAdded:Connect(function(d)
	if isProbe(d) then
		table.insert(log, { now(), "+", string.sub(d.Name, 3) })
	end
end)
workspace.DescendantRemoving:Connect(function(d)
	if isProbe(d) then
		table.insert(log, { now(), "-", string.sub(d.Name, 3) })
	end
end)
workspace.PersistentLoaded:Connect(function(player)
	table.insert(log, { now(), "persistentLoaded", player and player.Name or "nil" })
end)
local player = Players.LocalPlayer
player:GetPropertyChangedSignal("GameplayPaused"):Connect(function()
	table.insert(log, { now(), "paused", player.GameplayPaused })
end)

if not game:IsLoaded() then
	game.Loaded:Wait()
end
first.loaded = snapshot()
first.loadedAt = now()
