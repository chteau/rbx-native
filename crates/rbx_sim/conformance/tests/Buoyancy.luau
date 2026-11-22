-- Blocks in terrain water: 2x2x2 blocks of density 0.3, 0.7, 0.95 and 1.5
-- (water is 1) dropped from a stud above a lake 24 deep, a 4x1x4 plank of
-- 0.5 laid flat on it, and a Wood ball. Height of each centre above the
-- water's surface over time, when it settles, its sideways drift.
return function(h, origin)
	local terrain = workspace.Terrain
	local function at(x, y, z)
		return origin + Vector3.new(x, y, z)
	end
	-- Water's surface at origin.Y.
	terrain:FillBlock(CFrame.new(at(0, -12, 0)), Vector3.new(64, 24, 64), Enum.Material.Water)
	terrain:FillBlock(CFrame.new(at(0, -28, 0)), Vector3.new(64, 8, 64), Enum.Material.Rock)
	-- New terrain takes a few frames to collide.
	h.beats(30)
	local function body(name, x, size, density, props)
		local p = {
			Name = name,
			Size = size,
			Position = at(x, size.Y / 2 + 1, 0),
			CustomPhysicalProperties = PhysicalProperties.new(density, 0.3, 0.5),
		}
		for key, value in props or {} do
			p[key] = value
		end
		return h.part(p)
	end
	local parts = {
		body("d03", -24, Vector3.new(2, 2, 2), 0.3),
		body("d07", -12, Vector3.new(2, 2, 2), 0.7),
		body("d095", 0, Vector3.new(2, 2, 2), 0.95),
		body("d15", 12, Vector3.new(2, 2, 2), 1.5),
		body("plank", 24, Vector3.new(4, 1, 4), 0.5),
	}
	local wood = h.part({
		Name = "wood",
		Shape = Enum.PartType.Ball,
		Material = Enum.Material.Wood,
		Size = Vector3.new(2, 2, 2),
		Position = at(0, 2, 16),
	})
	table.insert(parts, wood)
	local starts = {}
	for _, p in parts do
		starts[p.Name] = p.Position
	end
	local result = {}
	local traces = {}
	local settled = {}
	local minY = {}
	for i = 1, 300 do
		h.beat()
		for _, p in parts do
			local y = p.Position.Y - origin.Y
			minY[p.Name] = math.min(minY[p.Name] or y, y)
			if i % 10 == 0 then
				traces[p.Name] = traces[p.Name] or {}
				table.insert(traces[p.Name], h.round(y, 2))
			end
			if i == 15 or i == 30 or i == 60 or i == 120 or i == 300 then
				result[p.Name .. "_y" .. i] = h.round(y, 2)
			end
			if not settled[p.Name] and i > 10 and p.AssemblyLinearVelocity.Magnitude < 0.05 then
				settled[p.Name] = i
			end
		end
	end
	local d15 = parts[4]
	result._d15_tilt = h.round(math.deg(math.acos(math.clamp(d15.CFrame.UpVector.Y, -1, 1))), 2)
	result._d15_at = h.vec(d15.Position - origin, 2)
	local floor = {}
	local params = RaycastParams.new()
	params.IgnoreWater = true
	params.FilterDescendantsInstances = parts
	for x = 8, 16 do
		local hit = workspace:Raycast(at(x + 0.5, 4, 0.5), Vector3.new(0, -60, 0), params)
		table.insert(floor, hit and string.format("%.2f", hit.Position.Y - origin.Y) or "miss")
	end
	result._floor = table.concat(floor, " ")
	for _, p in parts do
		result[p.Name .. "_min"] = h.round(minY[p.Name], 2)
		result[p.Name .. "_settled"] = settled[p.Name] or -1
		result[p.Name .. "_vy"] = h.round(p.AssemblyLinearVelocity.Y, 2)
		local d = p.Position - starts[p.Name]
		result[p.Name .. "_drift"] = h.round(Vector3.new(d.X, 0, d.Z).Magnitude, 2)
		result["_" .. p.Name] = traces[p.Name]
	end
	h.clear(parts)
	-- Waves off: the same 0.7 block again.
	local waveSize, waveSpeed = terrain.WaterWaveSize, terrain.WaterWaveSpeed
	result.wave_size = waveSize
	result.wave_speed = waveSpeed
	terrain.WaterWaveSize = 0
	terrain.WaterWaveSpeed = 0
	local calm = body("calm", -12, Vector3.new(2, 2, 2), 0.7)
	for i = 1, 120 do
		h.beat()
		if i == 60 or i == 120 then
			result["calm_y" .. i] = h.round(calm.Position.Y - origin.Y, 2)
		end
	end
	calm:Destroy()
	terrain.WaterWaveSize = waveSize
	terrain.WaterWaveSpeed = waveSpeed
	terrain:Clear()
	return result
end
