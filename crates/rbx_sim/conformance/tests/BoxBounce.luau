-- A plastic 2x2x2 block dropped flat from 11 studs onto a plastic floor:
-- its first bounces and whether it picks up spin. _y/_vy/_spin are
-- per-frame traces from the first contact on.
return function(h, origin)
	local floor = h.part({ Name = "Floor", Anchored = true, Size = Vector3.new(20, 1, 20), Position = origin })
	local block = h.part({ Name = "Block", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(0, 12, 0) })
	local rest = origin.Y + 1.5
	local ys, vys, spins = {}, {}, {}
	local first, peaks, rising, top = nil, {}, false, -math.huge
	local maxSpin, maxDrift = 0, 0
	for i = 1, 90 do
		h.beat()
		local v = block.AssemblyLinearVelocity
		local y = block.Position.Y - rest
		local spin = block.AssemblyAngularVelocity.Magnitude
		maxSpin = math.max(maxSpin, spin)
		local drift = Vector3.new(block.Position.X - origin.X, 0, block.Position.Z - origin.Z).Magnitude
		maxDrift = math.max(maxDrift, drift)
		if i >= 18 and i <= 70 then
			table.insert(ys, h.round(y, 3))
			table.insert(vys, h.round(v.Y, 2))
			table.insert(spins, h.round(spin, 2))
		end
		if not rising and v.Y > 0.5 then
			rising, top = true, y
			first = first or i
		elseif rising then
			top = math.max(top, y)
			if v.Y < 0 then
				rising = false
				table.insert(peaks, top)
			end
		end
	end
	floor:Destroy()
	block:Destroy()
	return {
		first_bounce_beat = first or -1,
		peak_1 = h.round(peaks[1] or -1, 3),
		peak_2 = h.round(peaks[2] or -1, 3),
		bounces = #peaks,
		max_spin = h.round(maxSpin, 2),
		max_drift = h.round(maxDrift, 3),
		_y = ys,
		_vy = vys,
		_spin = spins,
	}
end
