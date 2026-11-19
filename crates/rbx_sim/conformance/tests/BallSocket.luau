-- A bob on a BallSocketConstraint 4 studs from an anchored pivot,
-- released level with a 5 stud/s sideways push: position after 20 and 40.
return function(h, origin)
	local pivot, bob = h.rig(origin, { Name = "Bob", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(4, 0, 0) })
	h.new("BallSocketConstraint", {
		Attachment0 = h.attach(pivot),
		Attachment1 = h.attach(bob, CFrame.new(-4, 0, 0)),
	}, pivot)
	bob.AssemblyLinearVelocity = Vector3.new(0, 0, 5)
	local r = {}
	for i = 1, 40 do
		h.beat()
		if i == 20 or i == 40 then
			local at = bob.Position - origin
			r["x_" .. i], r["y_" .. i], r["z_" .. i] = h.round(at.X, 2), h.round(at.Y, 2), h.round(at.Z, 2)
			r["dist_" .. i] = h.round(at.Magnitude, 3)
		end
	end
	h.clear({ pivot, bob })
	return r
end
