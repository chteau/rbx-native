-- A falling block spun at 5 rad/s about Y by AngularVelocity (unlimited
-- torque): its spin after 1 and 30 frames and how far it fell.
return function(h, origin)
	local body = h.part({ Name = "Body", Size = Vector3.new(1, 1, 1), Position = origin })
	h.new("AngularVelocity", {
		Attachment0 = h.attach(body),
		AngularVelocity = Vector3.new(0, 5, 0),
		MaxTorque = math.huge,
	}, body)
	local r = {}
	for i = 1, 30 do
		h.beat()
		if i == 1 or i == 30 then
			r["spin_" .. i] = h.round(body.AssemblyAngularVelocity.Y, 3)
		end
	end
	r.dy_30 = h.round(body.Position.Y - origin.Y, 3)
	body:Destroy()
	return r
end
