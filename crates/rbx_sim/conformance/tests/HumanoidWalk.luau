-- An R15 standing on a floor walks with Humanoid:Move: how it speeds up
-- (WalkSpeed 16), slows down when the move stops, and the same at
-- WalkSpeed 32. MoveDirection read back right after Move.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(160, 2, 160))
	local model, humanoid, root = h.character(origin + Vector3.new(-40, 0, -40), "R15")
	h.beats(30)
	local result = {}
	local x0 = root.Position.X
	result.y_stand = h.round(root.Position.Y - origin.Y, 3)
	humanoid:Move(Vector3.new(1, 0, 0))
	result.movedir = h.vec(humanoid.MoveDirection, 3)
	local vx = {}
	for i = 1, 60 do
		h.beat()
		table.insert(vx, h.round(root.AssemblyLinearVelocity.X, 3))
		if i == 1 or i == 2 or i == 3 or i == 5 or i == 10 or i == 20 or i == 60 then
			result["vx_" .. i] = h.round(root.AssemblyLinearVelocity.X, 2)
		end
	end
	result.dx_60 = h.round(root.Position.X - x0, 2)
	result.y_walk = h.round(root.Position.Y - origin.Y, 3)
	result.vz_walk = h.round(root.AssemblyLinearVelocity.Z, 3)
	result.yaw_walk = h.yaw(root.CFrame)
	local speeds = {}
	humanoid.Running:Connect(function(speed)
		table.insert(speeds, h.round(speed, 2))
	end)
	humanoid:Move(Vector3.zero)
	result.movedir_stop = h.vec(humanoid.MoveDirection, 3)
	local x1 = root.Position.X
	local stop = {}
	for i = 1, 30 do
		h.beat()
		table.insert(stop, h.round(root.AssemblyLinearVelocity.X, 3))
		if i == 1 or i == 2 or i == 3 or i == 5 or i == 10 then
			result["stop_vx_" .. i] = h.round(root.AssemblyLinearVelocity.X, 2)
		end
	end
	result.stop_dx = h.round(root.Position.X - x1, 3)
	result.running = table.concat(speeds, ",")
	humanoid.WalkSpeed = 32
	humanoid:Move(Vector3.new(0, 0, 2))
	result.movedir_2 = h.vec(humanoid.MoveDirection, 3)
	local z0 = root.Position.Z
	local vz = {}
	for i = 1, 60 do
		h.beat()
		table.insert(vz, h.round(root.AssemblyLinearVelocity.Z, 3))
		if i == 1 or i == 3 or i == 5 or i == 10 or i == 20 or i == 60 then
			result["vz32_" .. i] = h.round(root.AssemblyLinearVelocity.Z, 2)
		end
	end
	result.dz32_60 = h.round(root.Position.Z - z0, 2)
	result.yaw_32 = h.yaw(root.CFrame)
	result._vx = vx
	result._stop = stop
	result._vz32 = vz
	model:Destroy()
	floor:Destroy()
	return result
end
