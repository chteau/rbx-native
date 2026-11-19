-- A 2x2x2 block dropped in empty space: how far it fell and how fast it
-- goes after 15, 30 and 60 heartbeats.
return function(h, origin)
	local part = h.part({
		Name = "Faller",
		Size = Vector3.new(2, 2, 2),
		Position = origin + Vector3.new(0, 200, 0),
	})
	local y0 = part.Position.Y
	local result = {}
	for i = 1, 60 do
		h.beat()
		if i == 15 or i == 30 or i == 60 then
			result["dy_" .. i] = h.round(part.Position.Y - y0, 3)
			result["vy_" .. i] = h.round(part.AssemblyLinearVelocity.Y, 3)
		end
	end
	part:Destroy()
	return result
end
