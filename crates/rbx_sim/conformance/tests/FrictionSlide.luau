-- A plastic block resting on a plastic floor, pushed to 20 studs/s along X:
-- its speed after 15 heartbeats, when it stops and how far it slid.
return function(h, origin)
	local floor = h.part({
		Name = "Floor",
		Anchored = true,
		Size = Vector3.new(200, 1, 20),
		Position = origin,
	})
	local block = h.part({
		Name = "Block",
		Size = Vector3.new(2, 2, 2),
		Position = origin + Vector3.new(-80, 1.5, 0),
	})
	h.beats(30)
	local x0 = block.Position.X
	block.AssemblyLinearVelocity = Vector3.new(20, 0, 0)
	local result = {}
	local stopped
	for i = 1, 120 do
		h.beat()
		local speed = block.AssemblyLinearVelocity.Magnitude
		if i == 15 then
			result.speed_15 = h.round(speed, 3)
		end
		if not stopped and speed < 0.01 then
			stopped = i
		end
	end
	result.stopped_beat = stopped or -1
	result.distance = h.round(block.Position.X - x0, 3)
	result.drift_z = h.round(block.Position.Z - origin.Z, 3)
	floor:Destroy()
	block:Destroy()
	return result
end
