-- A block hanging 2 studs under a pivot on a 6 stud RopeConstraint: it
-- falls slack until the rope catches it. Height after 10, 20 and 40 frames.
return function(h, origin)
	local pivot, bob = h.rig(origin, { Name = "Bob", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(0, -2, 0) })
	local rope = h.new("RopeConstraint", {
		Attachment0 = h.attach(pivot),
		Attachment1 = h.attach(bob),
		Length = 6,
	}, pivot)
	local r = {}
	for i = 1, 40 do
		h.beat()
		if i == 10 or i == 20 or i == 40 then
			r["y_" .. i] = h.round(bob.Position.Y - origin.Y, 3)
		end
	end
	r.distance_40 = h.round(rope.CurrentDistance, 3)
	h.clear({ pivot, bob })
	return r
end
