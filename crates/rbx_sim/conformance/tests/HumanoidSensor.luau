-- The floor sensor: R15s dropped with their feet 0.5 to 8 studs over a
-- floor (when Freefall starts, when Landed comes, the root every beat), and
-- an R15 walked off a ledge.
return function(h, origin)
	local floor = h.floor(origin)
	local clock = h.clock()
	local heights = { 0.5, 1, 1.5, 2, 3, 5, 8 }
	local rigs = {}
	for i, height in heights do
		local model, humanoid, root = h.character(origin + Vector3.new(-35 + i * 8, height, -20), "R15")
		local name = "d" .. string.gsub(tostring(height), "%.", "")
		table.insert(rigs, { name, model, humanoid, root })
	end
	local ledge = h.part({
		Name = "Ledge",
		Anchored = true,
		Size = Vector3.new(10, 6, 10),
		Position = origin + Vector3.new(0, 3, 20),
	})
	local walker, walkerHumanoid, walkerRoot = h.character(origin + Vector3.new(0, 6, 20), "R15")
	local result, logs, traces = {}, {}, {}
	for _, rig in rigs do
		logs[rig[1]] = h.states(rig[3], clock)
		traces[rig[1]] = {}
	end
	for _ = 1, 60 do
		clock.tick()
		for _, rig in rigs do
			table.insert(traces[rig[1]], h.round(rig[4].Position.Y - origin.Y, 3))
		end
	end
	for _, rig in rigs do
		result[rig[1] .. "_states"] = table.concat(logs[rig[1]], " ")
		result["_" .. rig[1]] = traces[rig[1]]
		rig[2]:Destroy()
	end
	local walkLog = h.states(walkerHumanoid, clock)
	local start = clock.now()
	walkerHumanoid:Move(Vector3.new(1, 0, 0))
	local walkTrace = {}
	for _ = 1, 50 do
		clock.tick()
		table.insert(walkTrace, string.format("%s/%s", h.round(walkerRoot.Position.X - origin.X, 2), h.round(walkerRoot.Position.Y - origin.Y, 2)))
	end
	local entries = {}
	for _, entry in walkLog do
		local old, new, beat = string.match(entry, "(%w+)>(%w+)@(%d+)")
		table.insert(entries, string.format("%s>%s@%d", old, new, tonumber(beat) - start))
	end
	result.ledge_states = table.concat(entries, " ")
	result._ledge = walkTrace
	walker:Destroy()
	ledge:Destroy()
	floor:Destroy()
	return result
end
