-- Model:MoveTo onto what's already there: a 2x2x2 block model moved into
-- a block, a ball (beside its centre, where only its bounding box is), a
-- wedge (over its low end), a block turned 45 degrees (in its bounding
-- box's corner), a non-colliding block, and terrain (solid and water).
-- How high the model's pivot lands above the target, each.
return function(h, origin)
	local result = {}
	local function model(at)
		local m = Instance.new("Model")
		local p = Instance.new("Part")
		p.Size = Vector3.new(2, 2, 2)
		p.Anchored = true
		p.CFrame = CFrame.new(at)
		p.Parent = m
		m.PrimaryPart = p
		m.Parent = workspace
		return m
	end
	local function case(name, obstacle, target)
		local m = model(origin + Vector3.new(0, 80, 0))
		m:MoveTo(target)
		result[name] = h.round(m:GetPivot().Position.Y - target.Y, 2)
		result[name .. "_x"] = h.round(m:GetPivot().Position.X - target.X, 2)
		m:Destroy()
		if obstacle then
			obstacle:Destroy()
		end
	end
	local base = origin + Vector3.new(0, 0, 0)
	case("empty", nil, base + Vector3.new(0, 30, 0))
	local block = h.new("Part", { Anchored = true, Size = Vector3.new(6, 4, 6), CFrame = CFrame.new(base) })
	case("block", block, base)
	local ball = h.new("Part", {
		Anchored = true,
		Shape = Enum.PartType.Ball,
		Size = Vector3.new(8, 8, 8),
		CFrame = CFrame.new(base),
	})
	case("ball_side", ball, base + Vector3.new(3.5, 3, 3.5))
	local ball2 = h.new("Part", {
		Anchored = true,
		Shape = Enum.PartType.Ball,
		Size = Vector3.new(8, 8, 8),
		CFrame = CFrame.new(base),
	})
	case("ball_top", ball2, base + Vector3.new(0, 2, 0))
	local wedge = h.new("WedgePart", { Anchored = true, Size = Vector3.new(6, 6, 6), CFrame = CFrame.new(base) })
	case("wedge_low", wedge, base + Vector3.new(0, -1, -2.5))
	local turned = h.new("Part", {
		Anchored = true,
		Size = Vector3.new(6, 4, 6),
		CFrame = CFrame.new(base) * CFrame.Angles(0, math.rad(45), 0),
	})
	case("turned_corner", turned, base + Vector3.new(3.6, 0, 3.6))
	local ghost = h.new("Part", {
		Anchored = true,
		CanCollide = false,
		Size = Vector3.new(6, 4, 6),
		CFrame = CFrame.new(base),
	})
	case("no_collide", ghost, base)
	-- Terrain inside its extents (the test's own corner can lie past
	-- 32000 studs), in a spot no other test uses.
	local terrain = workspace.Terrain
	base = Vector3.new(-1500, 300, -1500)
	terrain:FillBlock(CFrame.new(base), Vector3.new(12, 8, 12), Enum.Material.Grass)
	case("terrain", nil, base)
	terrain:FillBlock(CFrame.new(base), Vector3.new(12, 8, 12), Enum.Material.Air)
	terrain:FillBlock(CFrame.new(base), Vector3.new(12, 8, 12), Enum.Material.Water)
	case("water", nil, base)
	terrain:FillBlock(CFrame.new(base), Vector3.new(12, 8, 12), Enum.Material.Air)
	return result
end
