-- Characters on terrain: standing on a flat Grass slab (stand height,
-- FloorMaterial), walking +Z up 30 and 60 degree terrain wedges (the 60
-- one again with MaxSlopeAngle 45).
return function(h, origin)
	local terrain = workspace.Terrain
	local function at(x, y, z)
		return origin + Vector3.new(x, y, z)
	end
	terrain:FillBlock(CFrame.new(at(0, -8, 0)), Vector3.new(96, 16, 96), Enum.Material.Grass)
	local lanes = {
		{ "s30", 30, nil },
		{ "s60", 60, nil },
		{ "s60m", 60, 45 },
	}
	local rigs = {}
	for i, lane in lanes do
		local x = -40 + i * 20
		local height = 12
		local length = height / math.tan(math.rad(lane[2]))
		terrain:FillWedge(
			CFrame.new(at(x, height / 2, length / 2 + 4)),
			Vector3.new(12, height, length),
			Enum.Material.Rock
		)
	end
	-- New terrain takes a few frames to collide.
	h.beats(30)
	for i, lane in lanes do
		local x = -40 + i * 20
		local model, humanoid, root =
			h.character(at(x, 0, -4), "R15", { MaxSlopeAngle = lane[3] or 89 })
		table.insert(rigs, { lane[1], model, humanoid, root })
	end
	local model, humanoid, root = h.character(at(30, 0, -20), "R15")
	h.beats(30)
	local result = {}
	result.stand_y = h.round(root.Position.Y - origin.Y, 3)
	result.floor = humanoid.FloorMaterial.Name
	result.state = humanoid:GetState().Name
	local start = {}
	for _, rig in rigs do
		start[rig[1]] = rig[4].Position
		rig[3]:Move(Vector3.new(0, 0, 1))
	end
	local traces = {}
	for i = 1, 90 do
		h.beat()
		for _, rig in rigs do
			traces[rig[1]] = traces[rig[1]] or {}
			table.insert(traces[rig[1]], h.round(rig[4].Position.Y - start[rig[1]].Y, 2))
			if i == 45 then
				result[rig[1] .. "_v45"] = h.vec(rig[4].AssemblyLinearVelocity, 2)
				result[rig[1] .. "_floor45"] = rig[3].FloorMaterial.Name
			end
		end
	end
	for _, rig in rigs do
		local name, r = rig[1], rig[4]
		result[name .. "_dz"] = h.round(r.Position.Z - start[name].Z, 2)
		result[name .. "_rise"] = h.round(r.Position.Y - start[name].Y, 2)
		result[name .. "_state"] = rig[3]:GetState().Name
		result["_" .. name] = traces[name]
		rig[2]:Destroy()
	end
	model:Destroy()
	terrain:Clear()
	return result
end
