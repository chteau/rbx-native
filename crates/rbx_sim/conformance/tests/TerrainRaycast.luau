-- Spatial queries against terrain: rays onto a flat Grass slab, a Sand
-- ball and a lake (with and without IgnoreWater), a Spherecast and a
-- Blockcast, and whether the bounds queries return Terrain.
return function(h, origin)
	local terrain = workspace.Terrain
	local function at(x, y, z)
		return origin + Vector3.new(x, y, z)
	end
	-- Slab top at origin.Y, studs -32..32 on X and Z.
	terrain:FillBlock(CFrame.new(at(0, -8, 0)), Vector3.new(64, 16, 64), Enum.Material.Grass)
	terrain:FillBall(at(-16, 4, -16), 8, Enum.Material.Sand)
	-- A lake 8 deep over rock, next to the slab.
	terrain:FillBlock(CFrame.new(at(48, -12, 0)), Vector3.new(32, 8, 32), Enum.Material.Rock)
	terrain:FillBlock(CFrame.new(at(48, -4, 0)), Vector3.new(32, 8, 32), Enum.Material.Water)
	-- Slabs whose tops are 1 and 3 studs into a cell (it reads 0.75, 0.25
	-- full) and one 2 studs into it.
	terrain:FillBlock(CFrame.new(at(-48, -9, 0)), Vector3.new(16, 16, 16), Enum.Material.Grass)
	terrain:FillBlock(CFrame.new(at(-48, -11, 24)), Vector3.new(16, 16, 16), Enum.Material.Grass)
	terrain:FillBlock(CFrame.new(at(-48, -10, -24)), Vector3.new(16, 16, 16), Enum.Material.Grass)
	-- How many beats before a ray finds the new terrain.
	local ready = -1
	for i = 0, 60 do
		if workspace:Raycast(at(5, 20, 5), Vector3.new(0, -40, 0)) then
			ready = i
			break
		end
		h.beat()
	end
	h.beats(30)
	local function hit(from, dir, props)
		local params = RaycastParams.new()
		for key, value in props or {} do
			params[key] = value
		end
		local r = workspace:Raycast(from, dir, params)
		if not r then
			return "miss"
		end
		return string.format(
			"%s %s %s n=%s",
			r.Instance.Name,
			r.Material.Name,
			h.round(r.Distance, 2),
			h.vec(r.Normal, 2)
		)
	end
	local down = Vector3.new(0, -40, 0)
	local result = {
		flat = hit(at(5, 20, 5), down),
		ball_top = hit(at(-16, 20, -16), down),
		ball_side = hit(at(-30, 4, -16), Vector3.new(30, 0, 0)),
		ball_slant = hit(at(-12, 20, -12), down),
		water = hit(at(48, 20, 0), down),
		ignore_water = hit(at(48, 20, 0), down, { IgnoreWater = true }),
		top_1 = hit(at(-48, 20, 0), down),
		top_3 = hit(at(-48, 20, 24), down),
		top_2 = hit(at(-48, 20, -24), down),
		under = hit(at(5, -20, 5), Vector3.new(0, 10, 0)),
		up_from_inside = hit(at(5, -4, 5), Vector3.new(0, 30, 0)),
		excluded = hit(at(5, 20, 5), down, {
			FilterType = Enum.RaycastFilterType.Exclude,
			FilterDescendantsInstances = { terrain },
		}),
	}
	local sphere = workspace:Spherecast(at(5, 20, 5), 2, down)
	result.spherecast = sphere and string.format("%s %s", sphere.Instance.Name, h.round(sphere.Distance, 2)) or "miss"
	local block = workspace:Blockcast(CFrame.new(at(5, 20, 5)), Vector3.new(4, 2, 4), down)
	result.blockcast = block and string.format("%s %s", block.Instance.Name, h.round(block.Distance, 2)) or "miss"
	result.bounds = h.names(workspace:GetPartBoundsInBox(CFrame.new(at(0, 0, 0)), Vector3.new(8, 8, 8)))
	result.radius = h.names(workspace:GetPartBoundsInRadius(at(0, 0, 0), 6))
	result._ready = ready
	terrain:Clear()
	return result
end
