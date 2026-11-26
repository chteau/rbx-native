-- How the walk speeds up and turns for other rigs and speeds: an R6, a rig
-- seven times as dense, WalkSpeed 8 and 50, a walk on ice. Velocity and
-- facing every beat for 20 beats after Move(+X) from facing -Z.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(200, 2, 200))
	local ice = h.part({
		Name = "Ice",
		Anchored = true,
		Material = Enum.Material.Ice,
		Size = Vector3.new(60, 1, 10),
		Position = origin + Vector3.new(0, 0.5, 40),
	})
	local function heavy(model)
		for _, part in model:GetChildren() do
			if part:IsA("BasePart") then
				part.CustomPhysicalProperties = PhysicalProperties.new(7, 0.3, 0.5)
			end
		end
	end
	local configs = {
		{ "r6", "R6", {}, nil, 0 },
		{ "heavy", "R15", {}, heavy, 0 },
		{ "ws8", "R15", { WalkSpeed = 8 }, nil, 0 },
		{ "ws50", "R15", { WalkSpeed = 50 }, nil, 0 },
		{ "ice", "R15", {}, nil, 1 },
	}
	local rigs = {}
	for i, config in configs do
		local z = if config[1] == "ice" then 40 else -60 + i * 12
		local model, humanoid, root = h.character(origin + Vector3.new(-25, config[5], z), config[2], config[3])
		if config[4] then
			config[4](model)
		end
		table.insert(rigs, { config[1], model, humanoid, root })
	end
	h.beats(30)
	for _, rig in rigs do
		rig[3]:Move(Vector3.new(1, 0, 0))
	end
	local result = {}
	local v, yaw = {}, {}
	for i = 1, 20 do
		h.beat()
		for _, rig in rigs do
			v[rig[1]] = v[rig[1]] or {}
			yaw[rig[1]] = yaw[rig[1]] or {}
			table.insert(v[rig[1]], h.round(rig[4].AssemblyLinearVelocity.X, 3))
			table.insert(yaw[rig[1]], h.yaw(rig[4].CFrame))
			if i == 1 or i == 2 or i == 3 or i == 10 then
				result[rig[1] .. "_vx" .. i] = h.round(rig[4].AssemblyLinearVelocity.X, 2)
				result[rig[1] .. "_yaw" .. i] = h.yaw(rig[4].CFrame)
			end
		end
	end
	for _, rig in rigs do
		result["_" .. rig[1] .. "_vx"] = v[rig[1]]
		result["_" .. rig[1] .. "_yaw"] = yaw[rig[1]]
		rig[2]:Destroy()
	end
	ice:Destroy()
	floor:Destroy()
	return result
end
