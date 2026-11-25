-- Characters set down on a floor: where the root settles (height above the
-- floor's top), the state, what still collides. An R15 a hair above the
-- floor, an R6, an R15 with HipHeight 3 (legs too short for it) and an R15
-- started a stud too low (legs in the floor).
return function(h, origin)
	local floor = h.floor(origin)
	local clock = h.clock()
	local rigs = {
		{ "r15", h.character(origin + Vector3.new(-15, 0.05, 0), "R15") },
		{ "r6", h.character(origin + Vector3.new(-5, 0.05, 0), "R6") },
		{ "hh3", h.character(origin + Vector3.new(5, 1.05, 0), "R15", { HipHeight = 3 }) },
		{ "low", h.character(origin + Vector3.new(15, -1, 0), "R15") },
	}
	local result = {}
	local traces = {}
	for _, rig in rigs do
		local humanoid = rig[2]:FindFirstChildOfClass("Humanoid")
		result[rig[1] .. "_state0"] = humanoid:GetState().Name
		traces[rig[1]] = {}
		result[rig[1] .. "_log"] = h.states(humanoid, clock)
	end
	for i = 1, 90 do
		clock.tick()
		for _, rig in rigs do
			local root = rig[2].HumanoidRootPart
			if i <= 60 then
				table.insert(traces[rig[1]], h.round(root.Position.Y - origin.Y, 3))
			end
			if i == 1 then
				result[rig[1] .. "_state1"] = rig[2].Humanoid:GetState().Name
			end
		end
	end
	for _, rig in rigs do
		local name, model = rig[1], rig[2]
		local root, humanoid = model.HumanoidRootPart, model.Humanoid
		result[name .. "_y"] = h.round(root.Position.Y - origin.Y, 3)
		result[name .. "_vy"] = h.round(root.AssemblyLinearVelocity.Y, 3)
		result[name .. "_state"] = humanoid:GetState().Name
		result[name .. "_floor"] = humanoid.FloorMaterial.Name
		result[name .. "_upright"] = h.round(root.CFrame.UpVector.Y, 3)
		local collide = {}
		for _, part in model:GetChildren() do
			if part:IsA("BasePart") and part.CanCollide then
				table.insert(collide, part)
			end
		end
		result[name .. "_collide"] = h.names(collide)
		result[name .. "_log"] = table.concat(result[name .. "_log"], " ")
		result["_" .. name .. "_y"] = traces[name]
		model:Destroy()
	end
	floor:Destroy()
	return result
end
