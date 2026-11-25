-- A falling block pushed along X by a world-space VectorForce of 20 times
-- its mass: X and X velocity after 30 frames.
return function(h, origin)
	local body = h.part({ Name = "Body", Size = Vector3.new(1, 1, 1), Position = origin })
	h.new("VectorForce", {
		Attachment0 = h.attach(body),
		Force = Vector3.new(body:GetMass() * 20, 0, 0),
		RelativeTo = Enum.ActuatorRelativeTo.World,
	}, body)
	for _ = 1, 30 do
		h.beat()
	end
	local r = {
		x_30 = h.round(body.Position.X - origin.X, 3),
		vx_30 = h.round(body.AssemblyLinearVelocity.X, 3),
		mass = h.round(body:GetMass(), 4),
	}
	body:Destroy()
	return r
end
