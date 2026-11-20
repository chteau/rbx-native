-- A block falls through an anchored non-colliding sensor onto a floor:
-- every Touched and TouchEnded in the order they fired, with the heartbeat.
return function(h, origin)
	local floor = h.part({
		Name = "Floor",
		Anchored = true,
		Size = Vector3.new(20, 1, 20),
		Position = origin,
	})
	local sensor = h.part({
		Name = "Sensor",
		Anchored = true,
		CanCollide = false,
		Size = Vector3.new(6, 1, 6),
		Position = origin + Vector3.new(0, 6, 0),
	})
	local faller = h.part({
		Name = "Faller",
		Size = Vector3.new(2, 2, 2),
		Position = origin + Vector3.new(0, 12, 0),
	})
	local beat = 0
	local events = {}
	for _, part in { floor, sensor, faller } do
		part.Touched:Connect(function(other)
			table.insert(events, string.format("%s<%s@%d", part.Name, other.Name, beat))
		end)
		part.TouchEnded:Connect(function(other)
			table.insert(events, string.format("%s/%s@%d", part.Name, other.Name, beat))
		end)
	end
	local trace = {}
	for i = 1, 90 do
		beat = i
		h.beat()
		table.insert(trace, h.round(faller.Position.Y - origin.Y - 1.5, 3))
	end
	floor:Destroy()
	sensor:Destroy()
	faller:Destroy()
	return { events = table.concat(events, " "), _y = trace }
end
