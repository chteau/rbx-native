-- A 1x1x1 bob on a free HingeConstraint 5 studs from an anchored pivot,
-- axis along world Z, released level: where it is after 15 and 30 frames.
return function(h, origin)
	local pivot, bob = h.rig(origin, { Name = "Bob", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(5, 0, 0) })
	local axis = CFrame.fromMatrix(Vector3.zero, Vector3.zAxis, Vector3.yAxis)
	local hinge = h.new("HingeConstraint", {
		Attachment0 = h.attach(pivot, axis),
		Attachment1 = h.attach(bob, CFrame.new(-5, 0, 0) * axis),
	}, pivot)
	local r = {}
	for i = 1, 30 do
		h.beat()
		if i == 15 or i == 30 then
			local at = bob.Position - origin
			r["x_" .. i], r["y_" .. i] = h.round(at.X, 3), h.round(at.Y, 3)
		end
	end
	r.angle_30 = h.round(hinge.CurrentAngle, 2)
	h.clear({ pivot, bob })
	return r
end
