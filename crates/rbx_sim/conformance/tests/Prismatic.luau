-- A block on a horizontal PrismaticConstraint motor (6 stud/s, unlimited
-- force) through an anchored pivot: slide after 15 and 30 frames, and
-- whether gravity moved it off the axis.
return function(h, origin)
	local pivot, slider = h.rig(origin, { Name = "Slider", CanCollide = false, Size = Vector3.new(1, 1, 1), Position = origin })
	local prismatic = h.new("PrismaticConstraint", {
		Attachment0 = h.attach(pivot),
		Attachment1 = h.attach(slider),
		ActuatorType = Enum.ActuatorType.Motor,
		Velocity = 6,
		MotorMaxForce = math.huge,
	}, pivot)
	local r = {}
	for i = 1, 30 do
		h.beat()
		if i == 15 or i == 30 then
			r["x_" .. i] = h.round(slider.Position.X - origin.X, 3)
			r["position_" .. i] = h.round(prismatic.CurrentPosition, 3)
		end
	end
	r.y_30 = h.round(slider.Position.Y - origin.Y, 3)
	h.clear({ pivot, slider })
	return r
end
