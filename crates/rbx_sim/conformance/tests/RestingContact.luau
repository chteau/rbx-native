-- A block placed exactly on an anchored floor, and one dropped 2 studs onto
-- it: where each sits after two seconds, and whether it still moves.
return function(h, origin)
	local floor = h.part({
		Name = "Floor",
		Anchored = true,
		Size = Vector3.new(30, 1, 30),
		Position = origin,
	})
	local placed = h.part({
		Name = "Placed",
		Size = Vector3.new(4, 1, 2),
		Position = origin + Vector3.new(-6, 1, 0),
	})
	local dropped = h.part({
		Name = "Dropped",
		Size = Vector3.new(4, 1, 2),
		Position = origin + Vector3.new(6, 3, 0),
	})
	h.beats(120)
	local rest = origin.Y + 1
	local result = {
		placed_dy = h.round(placed.Position.Y - rest, 4),
		placed_speed = h.round(placed.AssemblyLinearVelocity.Magnitude, 4),
		dropped_dy = h.round(dropped.Position.Y - rest, 4),
		dropped_speed = h.round(dropped.AssemblyLinearVelocity.Magnitude, 4),
	}
	floor:Destroy()
	placed:Destroy()
	dropped:Destroy()
	return result
end
