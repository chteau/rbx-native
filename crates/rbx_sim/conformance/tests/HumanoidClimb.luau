-- An R15 walks (+X) into a 40 stud TrussPart and keeps pushing: the
-- states, how fast it climbs; then the move stops (does it hang on or
-- slide?). An R6 does the same next to it.
return function(h, origin)
	local floor = h.floor(origin)
	local clock = h.clock()
	local rigs = {}
	for i, rigType in { "R15", "R6" } do
		local z = -10 + (i - 1) * 20
		local truss = h.part({
			Name = "Truss",
			Anchored = true,
			Size = Vector3.new(2, 40, 2),
			Position = origin + Vector3.new(4, 20, z),
		}, "TrussPart")
		local model, humanoid, root = h.character(origin + Vector3.new(-2, 0, z), rigType)
		table.insert(rigs, { string.lower(rigType), model, humanoid, root, truss })
	end
	for _ = 1, 30 do
		clock.tick()
	end
	local result, logs, traces, y0 = {}, {}, {}, {}
	for _, rig in rigs do
		logs[rig[1]] = h.states(rig[3], clock)
		traces[rig[1]] = {}
		traces[rig[1] .. "_x"] = {}
		y0[rig[1]] = rig[4].Position.Y
		rig[3]:Move(Vector3.new(1, 0, 0))
	end
	local start = clock.now()
	for i = 1, 120 do
		clock.tick()
		for _, rig in rigs do
			local name, root = rig[1], rig[4]
			table.insert(traces[name], h.round(root.Position.Y - y0[name], 2))
			if i <= 30 then
				table.insert(traces[name .. "_x"], h.round(rig[5].Position.X - root.Position.X, 3))
			end
			if i == 60 or i == 90 then
				result[name .. "_vy" .. i] = h.round(root.AssemblyLinearVelocity.Y, 2)
			end
		end
	end
	for _, rig in rigs do
		local name = rig[1]
		result[name .. "_rise"] = h.round(rig[4].Position.Y - y0[name], 2)
		result[name .. "_state"] = rig[3]:GetState().Name
		result[name .. "_gap"] = h.round(rig[5].Position.X - rig[4].Position.X, 2)
		rig[3]:Move(Vector3.zero)
	end
	for i = 1, 30 do
		clock.tick()
		for _, rig in rigs do
			local name = rig[1]
			if i == 30 then
				result[name .. "_hang_vy"] = h.round(rig[4].AssemblyLinearVelocity.Y, 2)
				result[name .. "_hang_state"] = rig[3]:GetState().Name
			end
		end
	end
	for _, rig in rigs do
		local name = rig[1]
		local entries = {}
		for _, entry in logs[name] do
			local old, new, beat = string.match(entry, "(%w+)>(%w+)@(%d+)")
			table.insert(entries, string.format("%s>%s@%d", old, new, tonumber(beat) - start))
		end
		result[name .. "_states"] = table.concat(entries, " ")
		result["_" .. name .. "_y"] = traces[name]
		result["_" .. name .. "_x"] = traces[name .. "_x"]
		rig[2]:Destroy()
		rig[5]:Destroy()
	end
	floor:Destroy()
	return result
end
