-- Characters walk (+Z) up wedges: 30 and 60 degrees with the default
-- MaxSlopeAngle, 60 degrees with MaxSlopeAngle 45. Progress, rise, speed
-- along the slope, the state.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(160, 2, 160))
	local lanes = {
		{ "s30", 30, nil },
		{ "s60", 60, nil },
		{ "s60m", 60, 45 },
	}
	local rigs = {}
	for i, lane in lanes do
		local x = -30 + i * 15
		local height = 12
		local length = height / math.tan(math.rad(lane[2]))
		-- A WedgePart's vertical face is at +Z: the slope climbs along +Z.
		local wedge = h.part({
			Name = "Slope",
			Anchored = true,
			Size = Vector3.new(8, height, length),
		}, "WedgePart")
		wedge.CFrame = CFrame.new(origin + Vector3.new(x, height / 2, length / 2 + 2))
		local model, humanoid, root =
			h.character(origin + Vector3.new(x, 0, -4), "R15", { MaxSlopeAngle = lane[3] or 89 })
		table.insert(rigs, { lane[1], model, humanoid, root, wedge })
	end
	h.beats(30)
	local result = {}
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
				local v = rig[4].AssemblyLinearVelocity
				result[rig[1] .. "_v45"] = h.vec(v, 2)
			end
		end
	end
	for _, rig in rigs do
		local name, root = rig[1], rig[4]
		result[name .. "_dz"] = h.round(root.Position.Z - start[name].Z, 2)
		result[name .. "_rise"] = h.round(root.Position.Y - start[name].Y, 2)
		result[name .. "_state"] = rig[3]:GetState().Name
		result["_" .. name] = traces[name]
		rig[2]:Destroy()
		rig[5]:Destroy()
	end
	floor:Destroy()
	return result
end
