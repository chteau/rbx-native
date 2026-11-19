-- An R15 and an R6 dropped with their feet 20 studs over a floor: the state
-- sequence, the beat they land, where the root ends up.
return function(h, origin)
	local floor = h.floor(origin)
	local clock = h.clock()
	local rigs = {
		{ "r15", h.character(origin + Vector3.new(-5, 20, 0), "R15") },
		{ "r6", h.character(origin + Vector3.new(5, 20, 0), "R6") },
	}
	local result, traces, logs = {}, {}, {}
	for _, rig in rigs do
		traces[rig[1]] = {}
		logs[rig[1]] = h.states(rig[2].Humanoid, clock)
	end
	for i = 1, 100 do
		clock.tick()
		for _, rig in rigs do
			local root = rig[2].HumanoidRootPart
			table.insert(traces[rig[1]], h.round(root.Position.Y - origin.Y, 3))
			if i == 20 then
				result[rig[1] .. "_vy20"] = h.round(root.AssemblyLinearVelocity.Y, 2)
			end
		end
	end
	for _, rig in rigs do
		local name, model = rig[1], rig[2]
		result[name .. "_states"] = table.concat(logs[name], " ")
		result[name .. "_y"] = h.round(model.HumanoidRootPart.Position.Y - origin.Y, 3)
		result[name .. "_state"] = model.Humanoid:GetState().Name
		for i, y in traces[name] do
			if y < traces[name][1] - 15 and result[name .. "_land_beat"] == nil and i > 1 then
				local last = traces[name][i - 1]
				if math.abs(y - last) < 0.05 then
					result[name .. "_land_beat"] = i
				end
			end
		end
		result["_" .. name .. "_y"] = traces[name]
		model:Destroy()
	end
	floor:Destroy()
	return result
end
