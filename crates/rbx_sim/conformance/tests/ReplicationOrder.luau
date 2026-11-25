-- What the client sees of a frame's worth of changes (a model built,
-- changed, a child destroyed, renamed, all before it replicates), of an
-- instance moved out to ServerStorage and back; what never replicates
-- (ServerStorage, ServerScriptService, PlayerScripts, the client's own
-- writes); where Scripts, LocalScripts and RunContexts run; game.Loaded.
return function(h)
	local ReplicatedStorage = game:GetService("ReplicatedStorage")
	local ServerStorage = game:GetService("ServerStorage")
	local player = h.player()
	local result = {}
	local doomed = Instance.new("Folder")
	doomed.Name = "ClientDestroys"
	doomed.Parent = ReplicatedStorage
	h.client("ReplicationOrder", "setup")

	local model = Instance.new("Model")
	model.Name = "Batch"
	local p = Instance.new("Part")
	p.Name = "P"
	p.Anchored = true
	p.Parent = model
	model:SetAttribute("a", 1)
	model.Parent = workspace
	p.Transparency = 0.5
	local q = Instance.new("Part")
	q.Name = "Q"
	q.Anchored = true
	q.Parent = model
	local r = Instance.new("Part")
	r.Name = "R"
	r.Anchored = true
	r.Parent = model
	r:Destroy()
	model.Name = "Batch2"
	model:SetAttribute("a", 2)
	h.beats(10)
	-- Changes to it over three frames.
	p.Transparency = 0.25
	h.beat()
	q:Destroy()
	h.beat()
	model.Name = "Batch3"
	h.beats(10)

	local hop = Instance.new("Folder")
	hop.Name = "Hop"
	hop.Parent = ReplicatedStorage
	h.beats(10)
	hop.Parent = ServerStorage
	h.beats(10)
	hop.Parent = ReplicatedStorage
	h.beats(10)

	for key, value in h.client("ReplicationOrder", "report") do
		result[key] = value
	end
	result.server_baseplate_transparency = workspace.Baseplate.Transparency
	result.server_sees_client_part = workspace:FindFirstChild("ClientMade") ~= nil
	result.server_keeps_client_destroyed = ReplicatedStorage:FindFirstChild("ClientDestroys") ~= nil
	result.server_player_scripts = player:FindFirstChild("PlayerScripts") ~= nil
	result.server_backpack = player:FindFirstChild("Backpack") ~= nil
	result.server_player_gui = player:FindFirstChild("PlayerGui") ~= nil
	result.server_client_attribute = tostring(player:GetAttribute("FromClient"))
	result.server_rc_server_rs = _G.rc_server_rs == true
	result.server_legacy_rs = _G.legacy_rs == true
	result.server_rc_client_ws = _G.rc_client_ws == true
	result.server_local_ws = _G.local_ws == true
	model:Destroy()
	hop:Destroy()
	doomed:Destroy()
	return result
end
