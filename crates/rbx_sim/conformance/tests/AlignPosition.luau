-- A free block pulled 10 studs along X by a one-attachment AlignPosition
-- (unlimited force, responsiveness 10): X after 5, 15, 30 and 60 frames.
return function(h, origin)
	local body = h.part({ Name = "Body", Size = Vector3.new(1, 1, 1), Position = origin })
	h.new("AlignPosition", {
		Mode = Enum.PositionAlignmentMode.OneAttachment,
		Attachment0 = h.attach(body),
		Position = origin + Vector3.new(10, 0, 0),
		MaxForce = math.huge,
		Responsiveness = 10,
	}, body)
	local r = {}
	for i = 1, 60 do
		h.beat()
		if i == 5 or i == 15 or i == 30 or i == 60 then
			r["x_" .. i] = h.round(body.Position.X - origin.X, 3)
		end
	end
	r.y_60 = h.round(body.Position.Y - origin.Y, 3)
	body:Destroy()
	return r
end
