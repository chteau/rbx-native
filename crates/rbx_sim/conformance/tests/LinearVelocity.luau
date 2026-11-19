-- A free block held at 5 stud/s along X by LinearVelocity (unlimited
-- force): distance and velocity after 1 and 30 frames.
return function(h, origin)
	local body = h.part({ Name = "Body", Size = Vector3.new(1, 1, 1), Position = origin })
	h.new("LinearVelocity", {
		Attachment0 = h.attach(body),
		VectorVelocity = Vector3.new(5, 0, 0),
		MaxForce = math.huge,
	}, body)
	local r = {}
	for i = 1, 30 do
		h.beat()
		if i == 1 or i == 30 then
			local v = body.AssemblyLinearVelocity
			r["x_" .. i] = h.round(body.Position.X - origin.X, 3)
			r["y_" .. i] = h.round(body.Position.Y - origin.Y, 3)
			r["vx_" .. i], r["vy_" .. i] = h.round(v.X, 3), h.round(v.Y, 3)
		end
	end
	body:Destroy()
	return r
end
