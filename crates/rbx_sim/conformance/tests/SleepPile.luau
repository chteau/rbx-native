-- When loose parts go still. "Still" is a position that doesn't change
-- between frames (Roblox has no sleep API). Measured:
-- * frictionless blocks sliding on a floor at small speeds: which stop dead
--   (fall asleep while moving) and when;
-- * a pile of overlapping blocks dropped together, like a map's loose
--   props: when the last one goes still, how many still move later on.
return function(h, origin)
	local floor = h.part({ Name = "Floor", Anchored = true, Size = Vector3.new(120, 1, 60), Position = origin })
	local top = origin.Y + 0.5
	local r = {}
	local slick = PhysicalProperties.new(0.7, 0, 0, 100, 1)
	local speeds = { 0.05, 0.1, 0.2, 0.4, 0.8, 1.6, 3.2 }
	local sliders = {}
	for i, v in speeds do
		local b = h.part({
			Name = "Slider",
			Size = Vector3.new(2, 2, 2),
			Position = Vector3.new(origin.X - 50 + i * 6, top + 1, origin.Z - 20),
			CustomPhysicalProperties = slick,
		})
		sliders[i] = b
	end
	floor.CustomPhysicalProperties = slick
	h.beats(30)
	for i, v in speeds do
		sliders[i].AssemblyLinearVelocity = Vector3.new(0, 0, v)
	end
	local stopped, last = {}, {}
	for i, b in sliders do
		last[i] = b.Position
		stopped[i] = -1
	end
	for beat = 1, 180 do
		h.beat()
		for i, b in sliders do
			local p = b.Position
			if p == last[i] then
				if stopped[i] < 0 then
					stopped[i] = beat
				end
			else
				stopped[i] = -1
			end
			last[i] = p
		end
	end
	for i, v in speeds do
		r["slide_" .. v .. "_still_from"] = stopped[i]
		r["slide_" .. v .. "_moved"] = h.round(sliders[i].Position.Z - (origin.Z - 20), 2)
	end
	h.clear(sliders)
	floor.CustomPhysicalProperties = PhysicalProperties.new(Enum.Material.Plastic)

	-- The pile: 3 layers of 4x4 blocks, each overlapping its neighbours by
	-- a quarter stud, dropped from just above the floor.
	local pile = {}
	for layer = 0, 2 do
		for x = 0, 3 do
			for z = 0, 3 do
				local b = h.part({
					Name = "Pile",
					Size = Vector3.new(2, 1.5, 2),
					Position = Vector3.new(
						origin.X + 10 + x * 1.75 + layer * 0.3,
						top + 0.8 + layer * 1.3,
						origin.Z + z * 1.75 - layer * 0.2
					),
				})
				table.insert(pile, b)
			end
		end
	end
	local prev = {}
	local stillSince = {}
	for i, b in pile do
		prev[i] = b.Position
		stillSince[i] = -1
	end
	local allStill = -1
	for beat = 1, 300 do
		h.beat()
		local moving = 0
		for i, b in pile do
			local p = b.Position
			if p == prev[i] then
				if stillSince[i] < 0 then
					stillSince[i] = beat
				end
			else
				stillSince[i] = -1
				moving += 1
			end
			prev[i] = p
		end
		if moving == 0 and allStill < 0 then
			allStill = beat
		elseif moving > 0 then
			allStill = -1
		end
		if beat == 60 or beat == 120 or beat == 300 then
			r["pile_moving_" .. beat] = moving
		end
	end
	r.pile_all_still_from = allStill
	h.clear(pile)

	-- Deep overlaps, as in a prop built from loose overlapping parts: two
	-- 4-stud blocks sharing 1.5 studs, and two slabs in the same place
	-- crossed at right angles. How far apart they get and when they stop.
	local function pair(name, a, b)
		local p, q = h.part(a), h.part(b)
		local d0 = (p.Position - q.Position).Magnitude
		local lp, lq, still = p.Position, q.Position, -1
		for beat = 1, 300 do
			h.beat()
			if p.Position == lp and q.Position == lq then
				if still < 0 then
					still = beat
				end
			else
				still = -1
			end
			lp, lq = p.Position, q.Position
			if beat == 30 or beat == 300 then
				r[name .. "_apart_" .. beat] = h.round((p.Position - q.Position).Magnitude - d0, 2)
			end
		end
		r[name .. "_still_from"] = still
		h.clear({ p, q })
	end
	pair(
		"shared",
		{ Size = Vector3.new(4, 4, 4), Position = Vector3.new(origin.X + 30, top + 2, origin.Z) },
		{ Size = Vector3.new(4, 4, 4), Position = Vector3.new(origin.X + 32.5, top + 2, origin.Z) }
	)
	pair(
		"crossed",
		{ Size = Vector3.new(6, 1, 2), CFrame = CFrame.new(origin.X + 45, top + 0.5, origin.Z) },
		{
			Size = Vector3.new(6, 1, 2),
			CFrame = CFrame.new(origin.X + 45, top + 0.5, origin.Z) * CFrame.Angles(0, math.pi / 2, 0),
		}
	)
	-- Buried: a block made mostly inside an anchored slab is pushed out
	-- (a map piece sunk into the ground). Its rise over the first frames.
	local slab = h.part({
		Name = "Slab",
		Anchored = true,
		Size = Vector3.new(20, 8, 20),
		Position = Vector3.new(origin.X - 30, top + 4, origin.Z + 15),
	})
	for _, depth in { 2, 6 } do
		local b = h.part({
			Name = "Buried",
			Size = Vector3.new(8, 8, 8),
			Position = Vector3.new(origin.X - 30, top + 8 + 4 - depth, origin.Z + 15),
		})
		local y0 = b.Position.Y
		local lastY, still = y0, -1
		for beat = 1, 120 do
			h.beat()
			if beat == 1 or beat == 2 or beat == 4 or beat == 8 or beat == 15 or beat == 30 or beat == 120 then
				r["buried_" .. depth .. "_dy_" .. beat] = h.round(b.Position.Y - y0, 2)
			end
			if b.Position.Y == lastY then
				if still < 0 then
					still = beat
				end
			else
				still = -1
			end
			lastY = b.Position.Y
		end
		r["buried_" .. depth .. "_still_from"] = still
		h.clear({ b })
	end
	h.clear({ slab, floor })
	return r
end
