-- Characters jump from standing, all on the same beat: JumpPower 50
-- (UseJumpPower), JumpHeight 7.2 and 15 (UseJumpPower off), an R6 with
-- JumpPower 50, and an R15 that jumps while walking. Take-off speed, apex,
-- when they land, the states.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(160, 2, 160))
	local clock = h.clock()
	local rigs = {
		{ "power", h.character(origin + Vector3.new(-20, 0, 0), "R15", { UseJumpPower = true, JumpPower = 50 }) },
		{ "height", h.character(origin + Vector3.new(-10, 0, 0), "R15", { UseJumpPower = false, JumpHeight = 7.2 }) },
		{ "high", h.character(origin + Vector3.new(0, 0, 0), "R15", { UseJumpPower = false, JumpHeight = 15 }) },
		{ "r6", h.character(origin + Vector3.new(10, 0, 0), "R6", { UseJumpPower = true, JumpPower = 50 }) },
		{ "walk", h.character(origin + Vector3.new(20, 0, -30), "R15", { UseJumpPower = true, JumpPower = 50 }) },
	}
	for _ = 1, 30 do
		clock.tick()
	end
	local result, traces, logs, y0 = {}, {}, {}, {}
	rigs[5][3]:Move(Vector3.new(0, 0, 1))
	for _ = 1, 20 do
		clock.tick()
	end
	for _, rig in rigs do
		traces[rig[1]] = {}
		logs[rig[1]] = h.states(rig[3], clock)
		y0[rig[1]] = rig[4].Position.Y
		rig[3].Jump = true
	end
	result.jump_flag = rigs[1][3].Jump
	local start = clock.now()
	for i = 1, 80 do
		clock.tick()
		for _, rig in rigs do
			local name, humanoid, root = rig[1], rig[3], rig[4]
			local y = root.Position.Y - y0[name]
			table.insert(traces[name], h.round(y, 3))
			if i == 1 then
				result[name .. "_vy1"] = h.round(root.AssemblyLinearVelocity.Y, 2)
				result[name .. "_flag1"] = humanoid.Jump
			end
			if y > (result[name .. "_apex"] or -1) then
				result[name .. "_apex"] = h.round(y, 3)
				result[name .. "_apex_beat"] = i
			end
		end
	end
	for _, rig in rigs do
		local name = rig[1]
		result[name .. "_states"] = table.concat(logs[name], " ")
		result[name .. "_end"] = h.round(rig[4].Position.Y - y0[name], 3)
		for _, entry in logs[name] do
			local beat = string.match(entry, ">Landed@(%d+)")
			if beat and result[name .. "_land"] == nil then
				result[name .. "_land"] = tonumber(beat) - start
			end
		end
		result["_" .. name .. "_y"] = traces[name]
		rig[2]:Destroy()
	end
	floor:Destroy()
	return result
end
