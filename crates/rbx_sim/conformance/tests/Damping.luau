-- A block in the air spinning at 10 rad/s about Y and moving 20 stud/s
-- along X: whatever damping Roblox applies with nothing touching it.
return function(h, origin)
	local block = h.part({ Name = "Spinner", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(0, 100, 0) })
	block.AssemblyAngularVelocity = Vector3.new(0, 10, 0)
	block.AssemblyLinearVelocity = Vector3.new(20, 0, 0)
	local r = {}
	for i = 1, 120 do
		h.beat()
		if i == 30 or i == 60 or i == 120 then
			r["spin_" .. i] = h.round(block.AssemblyAngularVelocity.Y, 3)
			r["vx_" .. i] = h.round(block.AssemblyLinearVelocity.X, 3)
		end
	end
	block:Destroy()
	return r
end
