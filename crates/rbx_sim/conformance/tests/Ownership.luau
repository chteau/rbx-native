-- Network ownership: the character's, anchored parts', an unanchored part
-- far from and next to the player under auto, SetNetworkOwner and back to
-- auto, welded assemblies, and whose moves count: the client moving a
-- part it owns and one the server owns. Needs a player.
return function(h, origin)
	local ReplicatedStorage = game:GetService("ReplicatedStorage")
	local shared = require(ReplicatedStorage.ConformanceClient.Shared)
	local player = h.player()
	local character = player.Character or player.CharacterAdded:Wait()
	local root = character:WaitForChild("HumanoidRootPart")
	local result = {}
	local function owner(part)
		local ok, found = pcall(function()
			return part:GetNetworkOwner()
		end)
		if not ok then
			return "error"
		end
		return found == player and "player" or (found and found.Name or "server")
	end
	local function part(props)
		local p = Instance.new("Part")
		p.Size = Vector3.new(2, 2, 2)
		for key, value in props do
			p[key] = value
		end
		p.Parent = workspace
		return p
	end
	local floor = h.floor(origin)

	result.character = owner(root)
	result.character_auto = root:GetNetworkOwnershipAuto()

	local anchored = part({ Anchored = true, Position = origin + Vector3.new(0, 1, 0) })
	result.anchored_owner = owner(anchored)
	result.anchored_set = shared.err(function()
		anchored:SetNetworkOwner(nil)
	end)
	local can, why = anchored:CanSetNetworkOwnership()
	result.anchored_can = tostring(can) .. ":" .. tostring(why)

	local far = part({ Position = origin + Vector3.new(0, 1, 30) })
	h.beats(30)
	result.far_auto = far:GetNetworkOwnershipAuto()
	result.far_owner = owner(far)

	local near = part({ Size = Vector3.new(1, 1, 1), CFrame = root.CFrame * CFrame.new(4, 0, 0) })
	local trace = {}
	for i = 1, 90 do
		h.beat()
		if i == 1 or i == 10 or i == 30 or i == 90 then
			table.insert(trace, i .. ":" .. owner(near))
		end
	end
	result.near_trace = table.concat(trace, " ")

	far:SetNetworkOwner(player)
	result.set_owner = owner(far)
	result.set_auto = far:GetNetworkOwnershipAuto()
	far:SetNetworkOwnershipAuto()
	result.reset_auto = far:GetNetworkOwnershipAuto()
	h.beats(10)
	result.reset_owner = owner(far)

	local a = part({ Position = origin + Vector3.new(10, 1, 0) })
	local b = part({ Position = origin + Vector3.new(12, 1, 0) })
	local weld = Instance.new("WeldConstraint")
	weld.Part0 = a
	weld.Part1 = b
	weld.Parent = a
	a:SetNetworkOwner(player)
	result.weld_other = owner(b)
	weld:Destroy()
	h.beats(2)
	result.weld_broken_a = owner(a)
	result.weld_broken_b = owner(b)

	local stuck = part({ Position = origin + Vector3.new(-10, 1, 0) })
	local base = part({ Anchored = true, Position = origin + Vector3.new(-12, 1, 0) })
	local glue = Instance.new("WeldConstraint")
	glue.Part0 = stuck
	glue.Part1 = base
	glue.Parent = stuck
	result.anchored_weld_set = shared.err(function()
		stuck:SetNetworkOwner(player)
	end)

	local mine = part({ Position = origin + Vector3.new(0, 1, -10) })
	local theirs = part({ Position = origin + Vector3.new(6, 1, -10) })
	mine:SetNetworkOwner(player)
	theirs:SetNetworkOwner(nil)
	h.beats(30)
	h.client("Ownership", "move", mine, theirs)
	h.beats(30)
	result.client_moved_owned = shared.vec(mine.Position - origin)
	result.client_moved_server_owned = shared.vec(theirs.Position - origin)
	result.client_view_server_owned = h.client("Ownership", "where", theirs, origin)

	for _, p in { anchored, far, near, a, b, stuck, base, mine, theirs, floor } do
		p:Destroy()
	end
	return result
end
