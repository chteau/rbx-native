-- How take-off speed scales: JumpPower 30, 70 and 100, JumpHeight 3, 10 and
-- 25, a rig ten times as dense, and JumpPower 50 and JumpHeight 7.2 again
-- at half gravity. Take-off speed fitted by the caller from `_y`.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(160, 2, 160))
	local function heavy(model)
		for _, part in model:GetChildren() do
			if part:IsA("BasePart") then
				part.CustomPhysicalProperties = PhysicalProperties.new(7, 0.3, 0.5)
			end
		end
	end
	local function round(configs, beats)
		local rigs = {}
		for i, config in configs do
			local model, humanoid, root = h.character(origin + Vector3.new(-50 + i * 12, 0, 0), "R15", config[2])
			if config[3] then
				config[3](model)
			end
			table.insert(rigs, { config[1], model, humanoid, root })
		end
		h.beats(30)
		local y0, traces, out = {}, {}, {}
		for _, rig in rigs do
			y0[rig[1]] = rig[4].Position.Y
			traces[rig[1]] = {}
			rig[3].Jump = true
		end
		for _ = 1, beats do
			h.beat()
			for _, rig in rigs do
				local y = rig[4].Position.Y - y0[rig[1]]
				table.insert(traces[rig[1]], h.round(y, 3))
				out[rig[1] .. "_apex"] = math.max(out[rig[1] .. "_apex"] or 0, h.round(y, 3))
			end
		end
		for _, rig in rigs do
			out["_" .. rig[1] .. "_y"] = traces[rig[1]]
			rig[2]:Destroy()
		end
		return out
	end
	local result = round({
		{ "p30", { UseJumpPower = true, JumpPower = 30 } },
		{ "p70", { UseJumpPower = true, JumpPower = 70 } },
		{ "p100", { UseJumpPower = true, JumpPower = 100 } },
		{ "h3", { UseJumpPower = false, JumpHeight = 3 } },
		{ "h10", { UseJumpPower = false, JumpHeight = 10 } },
		{ "h25", { UseJumpPower = false, JumpHeight = 25 } },
		{ "heavy", { UseJumpPower = true, JumpPower = 50 }, heavy },
	}, 70)
	local gravity = workspace.Gravity
	workspace.Gravity = gravity / 2
	local half = round({
		{ "g_p50", { UseJumpPower = true, JumpPower = 50 } },
		{ "g_h72", { UseJumpPower = false, JumpHeight = 7.2 } },
	}, 90)
	workspace.Gravity = gravity
	for key, value in half do
		result[key] = value
	end
	floor:Destroy()
	return result
end
