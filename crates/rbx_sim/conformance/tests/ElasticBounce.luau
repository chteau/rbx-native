-- A plastic ball (elasticity 0.5) dropped 19 studs onto a plastic floor:
-- the heartbeat it first bounces at and the height of its first two peaks.
return function(h, origin)
	local floor = h.part({
		Name = "Floor",
		Anchored = true,
		Size = Vector3.new(30, 1, 30),
		Position = origin,
	})
	local ball = h.part({
		Name = "Ball",
		Shape = Enum.PartType.Ball,
		Size = Vector3.new(2, 2, 2),
		Position = origin + Vector3.new(0, 20.5, 0),
	})
	local rest = origin.Y + 1.5
	local bounces, peaks, first = 0, {}, nil
	local rising, top = false, -math.huge
	for i = 1, 240 do
		h.beat()
		local vy = ball.AssemblyLinearVelocity.Y
		local y = ball.Position.Y - rest
		if not rising and vy > 0.5 then
			rising, top = true, y
			bounces += 1
			first = first or i
		elseif rising then
			top = math.max(top, y)
			if vy < 0 then
				rising = false
				table.insert(peaks, top)
			end
		end
	end
	floor:Destroy()
	ball:Destroy()
	return {
		first_bounce_beat = first or -1,
		peak_1 = h.round(peaks[1] or -1, 3),
		peak_2 = h.round(peaks[2] or -1, 3),
	}
end
