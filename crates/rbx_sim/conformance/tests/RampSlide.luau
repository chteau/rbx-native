-- A plastic block set on a 30 degree anchored ramp (friction 0.3 < tan 30,
-- so it slides): distance down the slope after 15, 30 and 60 frames.
return function(h, origin)
	local tilt = CFrame.new(origin) * CFrame.Angles(0, 0, math.rad(30))
	local ramp = h.part({ Name = "Ramp", Anchored = true, Size = Vector3.new(60, 1, 10), CFrame = tilt })
	local block = h.part({ Name = "Block", Size = Vector3.new(2, 2, 2), CFrame = tilt * CFrame.new(10, 1.5, 0) })
	local start = block.Position
	local down = -tilt.RightVector
	local r = {}
	for i = 1, 60 do
		h.beat()
		if i == 15 or i == 30 or i == 60 then
			r["along_" .. i] = h.round((block.Position - start):Dot(down), 3)
		end
	end
	r.spin_60 = h.round(block.AssemblyAngularVelocity.Magnitude, 3)
	r.off_60 = h.round((block.Position - start):Dot(tilt.LookVector), 3)
	h.clear({ ramp, block })
	return r
end
