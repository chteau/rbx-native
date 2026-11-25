-- Humanoid:MoveTo: walking 20 studs to a point (the beat MoveToFinished
-- fires, where it stops), then a point behind a wall it can't get past
-- (MoveToFinished(false) after the timeout).
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(160, 2, 160))
	local model, humanoid, root = h.character(origin + Vector3.new(-40, 0, 0), "R15")
	local clock = h.clock()
	for _ = 1, 30 do
		clock.tick()
	end
	local result = {}
	local finished = {}
	humanoid.MoveToFinished:Connect(function(reached)
		table.insert(finished, string.format("%s@%d", tostring(reached), clock.now()))
	end)
	local start = clock.now()
	local target = root.Position + Vector3.new(20, 0, 0)
	humanoid:MoveTo(target)
	result.walkto = h.vec(humanoid.WalkToPoint - origin, 2)
	result.movedir = h.vec(humanoid.MoveDirection, 3)
	local vx = {}
	while #finished == 0 and clock.now() - start < 200 do
		clock.tick()
		table.insert(vx, h.round(root.AssemblyLinearVelocity.X, 2))
	end
	result.arrive = finished[1] and (clock.now() - start) or "never"
	result.arrive_dx = h.round(root.Position.X - target.X, 2)
	result.arrive_vx = h.round(root.AssemblyLinearVelocity.X, 2)
	result.movedir_after = h.vec(humanoid.MoveDirection, 3)
	for _ = 1, 30 do
		clock.tick()
	end
	result.rest_dx = h.round(root.Position.X - target.X, 2)

	local wall = h.part({
		Name = "Wall",
		Anchored = true,
		Size = Vector3.new(2, 40, 40),
		Position = root.Position + Vector3.new(8, 17, 0),
	})
	start = clock.now()
	humanoid:MoveTo(root.Position + Vector3.new(30, 0, 0))
	while #finished < 2 and clock.now() - start < 700 do
		clock.tick()
	end
	result.timeout = finished[2] and (clock.now() - start) or "never"
	result.blocked_dx = h.round(wall.Position.X - root.Position.X, 2)
	result.finished = table.concat(finished, " ")
	result._vx = vx
	wall:Destroy()
	-- How close counts as there: targets 0.5, 1.2 and 2 studs away.
	for _, d in { 0.5, 1.2, 2 } do
		local n = #finished
		start = clock.now()
		humanoid:MoveTo(root.Position + Vector3.new(0, 0, d))
		while #finished == n and clock.now() - start < 60 do
			clock.tick()
		end
		result["near_" .. d * 10] = if #finished > n then clock.now() - start else "never"
		for _ = 1, 10 do
			clock.tick()
		end
	end
	model:Destroy()
	floor:Destroy()
	return result
end
