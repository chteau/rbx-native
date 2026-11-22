-- Parts on terrain: a block and a ball resting on a flat slab, a ball
-- rolling down a 30 degree terrain wedge, blocks pushed to 20 stud/s along
-- Grass and Ice, a ball dropped onto each.
return function(h, origin)
	local terrain = workspace.Terrain
	local function at(x, y, z)
		return origin + Vector3.new(x, y, z)
	end
	-- Grass lane at X < 0, Ice lane at X > 0, tops at origin.Y.
	terrain:FillBlock(CFrame.new(at(-16, -8, 0)), Vector3.new(32, 16, 192), Enum.Material.Grass)
	terrain:FillBlock(CFrame.new(at(16, -8, 0)), Vector3.new(32, 16, 192), Enum.Material.Ice)
	-- A wedge 32 long rising 18.5 (30 degrees) toward +Z, its low end at -Z.
	local rise = 32 * math.tan(math.rad(30))
	terrain:FillWedge(
		CFrame.new(at(-16, rise / 2, -112)),
		Vector3.new(24, rise, 32),
		Enum.Material.Rock
	)
	terrain:FillBlock(CFrame.new(at(-16, -8, -112)), Vector3.new(24, 16, 32), Enum.Material.Rock)
	-- New terrain takes a few frames to collide.
	h.beats(30)
	local function block(name, x, z, props)
		local p = { Name = name, Size = Vector3.new(2, 2, 2), Position = at(x, 1, z) }
		for key, value in props or {} do
			p[key] = value
		end
		return h.part(p)
	end
	local placed = block("Placed", -8, 60)
	local ball = block("Ball", -12, 60, { Shape = Enum.PartType.Ball })
	local grass = block("GrassSlider", -8, 0)
	local ice = block("IceSlider", 8, 0)
	local grassDrop = block("GrassDrop", -20, 40, { Shape = Enum.PartType.Ball, Position = at(-20, 11, 40) })
	local iceDrop = block("IceDrop", 20, 40, { Shape = Enum.PartType.Ball, Position = at(20, 11, 40) })
	-- On the slope, 8 studs in from its high end: it rolls toward -Z.
	local roller = block("Roller", -16, -100, {
		Shape = Enum.PartType.Ball,
		Position = at(-16, rise * (1 - 8 / 32) + 2, -104),
	})
	local result = {}
	local peaks = { GrassDrop = 0, IceDrop = 0 }
	local fell = { GrassDrop = false, IceDrop = false }
	local function track()
		for _, p in { grassDrop, iceDrop } do
			local y = p.Position.Y - origin.Y - 1
			if p.AssemblyLinearVelocity.Y > 0 then
				fell[p.Name] = true
			end
			if fell[p.Name] then
				peaks[p.Name] = math.max(peaks[p.Name], y)
			end
		end
	end
	local rollTrace = {}
	local ballTrace = {}
	for i = 1, 30 do
		h.beat()
		track()
		table.insert(rollTrace, h.round(roller.Position.Z - origin.Z, 2))
		table.insert(ballTrace, h.round(ball.Position.Y - origin.Y, 2))
	end
	result.placed_y = h.round(placed.Position.Y - origin.Y, 3)
	result.ball_y = h.round(ball.Position.Y - origin.Y, 3)
	result.roll_z30 = h.round(roller.Position.Z - origin.Z, 2)
	result.roll_v30 = h.round(roller.AssemblyLinearVelocity.Magnitude, 2)
	local x0 = { GrassSlider = grass.Position.X, IceSlider = ice.Position.X }
	for _, p in { grass, ice } do
		p.AssemblyLinearVelocity = Vector3.new(0, 0, 20)
	end
	local z0 = { GrassSlider = grass.Position.Z, IceSlider = ice.Position.Z }
	local stopped = {}
	local iceTrace = {}
	for i = 1, 150 do
		h.beat()
		track()
		if i <= 30 then
			table.insert(rollTrace, h.round(roller.Position.Z - origin.Z, 2))
		end
		if i <= 80 then
			table.insert(iceTrace, h.round(ice.AssemblyLinearVelocity.Z, 2))
		end
		for _, p in { grass, ice } do
			local speed = p.AssemblyLinearVelocity.Magnitude
			if i == 15 then
				result[p.Name .. "_v15"] = h.round(speed, 2)
			end
			if not stopped[p.Name] and speed < 0.01 then
				stopped[p.Name] = i
			end
		end
	end
	for _, p in { grass, ice } do
		result[p.Name .. "_dist"] = h.round(p.Position.Z - z0[p.Name], 2)
		result[p.Name .. "_drift"] = h.round(p.Position.X - x0[p.Name], 2)
		result[p.Name .. "_stopped"] = stopped[p.Name] or -1
	end
	result.roll_z60 = h.round(roller.Position.Z - origin.Z, 2)
	result.grass_bounce = h.round(peaks.GrassDrop, 2)
	result.ice_bounce = h.round(peaks.IceDrop, 2)
	result._roll = rollTrace
	result._ball = ballTrace
	result._ice = iceTrace
	h.clear({ placed, ball, grass, ice, grassDrop, iceDrop, roller })
	terrain:Clear()
	return result
end
