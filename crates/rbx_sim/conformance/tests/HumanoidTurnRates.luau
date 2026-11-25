-- How fast AutoRotate turns: R15s told to Move along +X from facing -Z
-- (default, density 2 and 20, WalkSpeed 24, 32 and 0.01), one along +Z
-- (180 degrees), one along (1, 0, -1) (45). Facing, speed and spin every
-- beat for 25 beats.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(200, 2, 200))
	local function dense(d)
		return function(model)
			for _, part in model:GetChildren() do
				if part:IsA("BasePart") then
					part.CustomPhysicalProperties = PhysicalProperties.new(d, 0.3, 0.5)
				end
			end
		end
	end
	local configs = {
		{ "base", {}, nil, Vector3.new(1, 0, 0) },
		{ "d2", {}, dense(2), Vector3.new(1, 0, 0) },
		{ "d20", {}, dense(20), Vector3.new(1, 0, 0) },
		{ "ws24", { WalkSpeed = 24 }, nil, Vector3.new(1, 0, 0) },
		{ "ws32", { WalkSpeed = 32 }, nil, Vector3.new(1, 0, 0) },
		{ "ws0", { WalkSpeed = 0.01 }, nil, Vector3.new(1, 0, 0) },
		{ "back", {}, nil, Vector3.new(0, 0, 1) },
		{ "diag", {}, nil, Vector3.new(1, 0, -1).Unit },
	}
	local rigs = {}
	for i, c in configs do
		local model, humanoid, root = h.character(origin + Vector3.new(-70, 0, -80 + i * 16), "R15", c[2])
		if c[3] then
			c[3](model)
		end
		table.insert(rigs, { c[1], model, humanoid, root, c[4] })
	end
	h.beats(30)
	local result = {}
	for _, rig in rigs do
		rig[3]:Move(rig[5])
		result["_" .. rig[1] .. "_yaw"], result["_" .. rig[1] .. "_v"], result["_" .. rig[1] .. "_w"] = {}, {}, {}
	end
	for i = 1, 25 do
		h.beat()
		for _, rig in rigs do
			local name, root = rig[1], rig[4]
			table.insert(result["_" .. name .. "_yaw"], h.yaw(root.CFrame))
			table.insert(result["_" .. name .. "_v"], h.round(root.AssemblyLinearVelocity.Magnitude, 2))
			table.insert(result["_" .. name .. "_w"], h.round(root.AssemblyAngularVelocity.Y, 3))
			if i == 1 or i == 3 or i == 10 then
				result[name .. "_yaw" .. i] = h.yaw(root.CFrame)
			end
		end
	end
	for _, rig in rigs do
		rig[2]:Destroy()
	end
	floor:Destroy()
	return result
end
