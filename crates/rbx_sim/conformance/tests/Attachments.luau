-- An attachment's world frame on a turned part, and its axes.
return function(h, origin)
	local part = h.part({
		Name = "Holder",
		Anchored = true,
		Size = Vector3.new(4, 2, 2),
		CFrame = CFrame.new(origin) * CFrame.Angles(0, math.rad(90), math.rad(30)),
	})
	local a = h.attach(part, CFrame.new(1, 0.5, 0) * CFrame.Angles(math.rad(45), 0, 0))
	local r = {
		world_position = h.vec(a.WorldPosition - origin, 3),
		world_axis = h.vec(a.WorldAxis, 3),
		world_secondary = h.vec(a.WorldSecondaryAxis, 3),
		axis = h.vec(a.Axis, 3),
		secondary = h.vec(a.SecondaryAxis, 3),
		orientation = h.vec(a.Orientation, 2),
		world_orientation = h.vec(a.WorldOrientation, 2),
		position = h.vec(a.Position, 3),
	}
	a.WorldPosition = origin + Vector3.new(0, 3, 0)
	r.after_world_write = h.vec(a.Position, 3)
	a.Axis = Vector3.new(0, 0, 1)
	r.after_axis_write = h.vec(a.SecondaryAxis, 3)
	part:Destroy()
	return r
end
