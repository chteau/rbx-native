-- A falling block turned 90 degrees about Y by a one-attachment
-- AlignOrientation (unlimited torque, responsiveness 10): yaw in degrees.
return function(h, origin)
	local body = h.part({ Name = "Body", Size = Vector3.new(1, 1, 1), Position = origin })
	h.new("AlignOrientation", {
		Mode = Enum.OrientationAlignmentMode.OneAttachment,
		Attachment0 = h.attach(body),
		CFrame = CFrame.Angles(0, math.rad(90), 0),
		MaxTorque = math.huge,
		Responsiveness = 10,
	}, body)
	local r = {}
	for i = 1, 60 do
		h.beat()
		if i == 5 or i == 15 or i == 30 or i == 60 then
			local _, yaw = body.CFrame:ToOrientation()
			r["yaw_" .. i] = h.round(math.deg(yaw), 2)
		end
	end
	body:Destroy()
	return r
end
