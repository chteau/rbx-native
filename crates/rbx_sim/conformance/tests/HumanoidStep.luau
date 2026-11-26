-- Characters walk (+X) into blocks of different heights, each in its own
-- lane: how high they got (root rise) and how far, after 90 beats.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(160, 2, 160))
	local lanes = {
		{ "r15_05", "R15", 0.5 },
		{ "r15_1", "R15", 1 },
		{ "r15_15", "R15", 1.5 },
		{ "r15_2", "R15", 2 },
		{ "r15_25", "R15", 2.5 },
		{ "r15_35", "R15", 3.5 },
		{ "r6_1", "R6", 1 },
		{ "r6_2", "R6", 2 },
	}
	local rigs = {}
	for i, lane in lanes do
		local z = -42 + i * 10
		h.part({
			Name = "Step",
			Anchored = true,
			Size = Vector3.new(30, lane[3], 8),
			Position = origin + Vector3.new(20, lane[3] / 2, z),
		})
		local model, humanoid, root = h.character(origin + Vector3.new(-5, 0, z), lane[2])
		table.insert(rigs, { lane[1], model, humanoid, root, root.Position })
	end
	h.beats(30)
	for _, rig in rigs do
		rig[5] = rig[4].Position
		rig[3]:Move(Vector3.new(1, 0, 0))
	end
	local result = {}
	local traces = { r15_15 = {}, r15_25 = {}, r6_2 = {} }
	for _ = 1, 90 do
		h.beat()
		for _, rig in rigs do
			if traces[rig[1]] then
				table.insert(traces[rig[1]], h.round(rig[4].Position.Y - rig[5].Y, 3))
			end
		end
	end
	for _, rig in rigs do
		result[rig[1] .. "_rise"] = h.round(rig[4].Position.Y - rig[5].Y, 2)
		result[rig[1] .. "_dx"] = h.round(rig[4].Position.X - rig[5].X, 1)
		rig[2]:Destroy()
	end
	for name, trace in traces do
		result["_" .. name] = trace
	end
	for _, part in workspace:GetChildren() do
		if part.Name == "Step" then
			part:Destroy()
		end
	end
	floor:Destroy()
	return result
end
