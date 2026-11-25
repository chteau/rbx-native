-- A block on a SpringConstraint (free length 4, stiffness 200, damping 2)
-- released at the free length under a pivot: height after 10, 20, 40, 80.
return function(h, origin)
	local pivot, bob = h.rig(origin, { Name = "Bob", Size = Vector3.new(1, 1, 1), Position = origin + Vector3.new(0, -4, 0) })
	local spring = h.new("SpringConstraint", {
		Attachment0 = h.attach(pivot),
		Attachment1 = h.attach(bob),
		FreeLength = 4,
		Stiffness = 200,
		Damping = 2,
	}, pivot)
	local r = {}
	for i = 1, 80 do
		h.beat()
		if i == 10 or i == 20 or i == 40 or i == 80 then
			r["y_" .. i] = h.round(bob.Position.Y - origin.Y, 3)
		end
	end
	r.length_80 = h.round(spring.CurrentLength, 3)
	h.clear({ pivot, bob })
	return r
end
