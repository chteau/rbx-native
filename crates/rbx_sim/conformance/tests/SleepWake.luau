-- A block resting on a floor goes still; an impulse wakes it; a block
-- resting on a block that's destroyed falls. Roblox has no script API for
-- sleep, so "still" is the position not changing at all between frames.
return function(h, origin)
	local floor = h.part({ Name = "Floor", Anchored = true, Size = Vector3.new(30, 1, 30), Position = origin })
	local block = h.part({ Name = "Block", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(0, 1.5, 0) })
	local lower = h.part({ Name = "Lower", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(8, 1.5, 0) })
	local upper = h.part({ Name = "Upper", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(8, 3.5, 0) })
	local stillAt, last = -1, block.Position
	for i = 1, 120 do
		h.beat()
		if block.Position == last and block.AssemblyLinearVelocity == Vector3.zero then
			if stillAt < 0 then
				stillAt = i
			end
		else
			stillAt = -1
		end
		last = block.Position
	end
	local r = { still_from = stillAt, speed_120 = h.round(block.AssemblyLinearVelocity.Magnitude, 4) }
	local y0 = upper.Position.Y
	lower:Destroy()
	block:ApplyImpulse(Vector3.new(block:GetMass() * 10, 0, 0))
	local x0 = block.Position.X
	for i = 1, 30 do
		h.beat()
		if i == 1 then
			r.woke_vx_1 = h.round(block.AssemblyLinearVelocity.X, 2)
		end
	end
	r.woke_dx_30 = h.round(block.Position.X - x0, 2)
	r.upper_dy_30 = h.round(upper.Position.Y - y0, 2)
	h.clear({ floor, block, upper })
	return r
end
