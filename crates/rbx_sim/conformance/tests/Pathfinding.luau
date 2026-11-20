-- PathfindingService on a server, over anchored courses at the test's
-- origin: a straight walk, a wall to go around, a doorway too narrow for a
-- wide agent, a gap to jump, a ledge to climb (and not without jumping),
-- an unreachable island, a goal inside a part, a Neon strip with a cost,
-- a PathfindingModifier (a label with a cost, and PassThrough), a
-- PathfindingLink, WaypointSpacing, and Blocked when a wall lands on a
-- computed path. Per path: Status, waypoint count, length along the
-- waypoints, the first and last waypoint and the actions and labels.
return function(h, origin)
	local PathfindingService = game:GetService("PathfindingService")
	local result = {}
	local made = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local function part(props)
		local p = Instance.new("Part")
		p.Anchored = true
		p.TopSurface = Enum.SurfaceType.Smooth
		p.BottomSurface = Enum.SurfaceType.Smooth
		for k, v in props do
			p[k] = v
		end
		p.Parent = workspace
		table.insert(made, p)
		return p
	end
	-- A 60 x 1 x 60 floor whose top is at `y = 0` of the course.
	local function floor(at, size)
		return part({ Size = size or Vector3.new(60, 1, 60), Position = at - Vector3.new(0, 0.5, 0) })
	end
	local function run(name, at, from, to, params)
		local path = PathfindingService:CreatePath(params)
		local ok, message = pcall(function()
			path:ComputeAsync(at + from, at + to)
		end)
		result[name .. "_status"] = ok and path.Status.Name or "error: " .. tostring(message)
		local waypoints = path:GetWaypoints()
		result[name .. "_count"] = #waypoints
		local length = 0
		local actions = {}
		local labels = {}
		for i, w in waypoints do
			if i > 1 then
				length += (w.Position - waypoints[i - 1].Position).Magnitude
			end
			if w.Action ~= Enum.PathWaypointAction.Walk then
				table.insert(actions, i .. ":" .. w.Action.Name)
			end
			if w.Label ~= "" then
				table.insert(labels, i .. ":" .. w.Label)
			end
		end
		result[name .. "_length"] = math.floor(length * 100 + 0.5) / 100
		if #waypoints > 0 then
			local first = waypoints[1].Position - at
			local last = waypoints[#waypoints].Position - at
			result[name .. "_first_x"] = math.floor(first.X * 100 + 0.5) / 100
			result[name .. "_first_y"] = math.floor(first.Y * 100 + 0.5) / 100
			result[name .. "_first_z"] = math.floor(first.Z * 100 + 0.5) / 100
			result[name .. "_last_x"] = math.floor(last.X * 100 + 0.5) / 100
			result[name .. "_last_y"] = math.floor(last.Y * 100 + 0.5) / 100
			result[name .. "_last_z"] = math.floor(last.Z * 100 + 0.5) / 100
			local trace = {}
			for _, w in waypoints do
				local p = w.Position - at
				table.insert(trace, string.format("%.1f,%.1f,%.1f", p.X, p.Y, p.Z))
			end
			result["_" .. name .. "_trace"] = table.concat(trace, " ")
		end
		result[name .. "_actions"] = table.concat(actions, " ")
		result[name .. "_labels"] = table.concat(labels, " ")
		return path
	end
	local step = 80
	local function course(i)
		return origin + Vector3.new(0, 0, (i - 4) * step) - Vector3.new(0, origin.Y - 20, 0)
	end

	-- 1. Straight.
	local at = course(1)
	floor(at)
	run("straight", at, Vector3.new(-20, 3, 0), Vector3.new(20, 3, 0))
	run("spacing8", at, Vector3.new(-20, 3, 0), Vector3.new(20, 3, 0), { WaypointSpacing = 8 })
	run("spacing_huge", at, Vector3.new(-20, 3, 0), Vector3.new(20, 3, 0), { WaypointSpacing = math.huge })

	-- 2. A wall across the middle, open at +z.
	at = course(2)
	floor(at)
	part({ Size = Vector3.new(2, 10, 40), Position = at + Vector3.new(0, 5, -10) })
	run("wall", at, Vector3.new(-15, 3, -10), Vector3.new(15, 3, -10))
	-- A doorway 3 studs wide in a full wall: fits the default agent (radius
	-- 2 needs 4?), not a radius 3 one.
	at = course(3)
	floor(at)
	part({ Size = Vector3.new(2, 10, 28.5), Position = at + Vector3.new(0, 5, -15.75) })
	part({ Size = Vector3.new(2, 10, 28.5), Position = at + Vector3.new(0, 5, 15.75) })
	run("door_default", at, Vector3.new(-15, 3, 0), Vector3.new(15, 3, 0))
	run("door_wide", at, Vector3.new(-15, 3, 0), Vector3.new(15, 3, 0), { AgentRadius = 4 })

	-- 4. A 6 stud gap between two floors.
	at = course(4)
	part({ Size = Vector3.new(27, 1, 20), Position = at + Vector3.new(-16.5, -0.5, 0) })
	part({ Size = Vector3.new(27, 1, 20), Position = at + Vector3.new(16.5, -0.5, 0) })
	run("gap", at, Vector3.new(-20, 3, 0), Vector3.new(20, 3, 0))
	run("gap_nojump", at, Vector3.new(-20, 3, 0), Vector3.new(20, 3, 0), { AgentCanJump = false })

	-- 5. A 3 stud ledge.
	at = course(5)
	floor(at)
	part({ Size = Vector3.new(20, 3, 30), Position = at + Vector3.new(15, 1.5, 0) })
	run("ledge", at, Vector3.new(-20, 3, 0), Vector3.new(20, 6, 0))
	run("ledge_nojump", at, Vector3.new(-20, 3, 0), Vector3.new(20, 6, 0), { AgentCanJump = false })
	-- An unreachable island, and a goal inside a solid block.
	part({ Size = Vector3.new(10, 1, 10), Position = at + Vector3.new(0, 40, 0) })
	run("island", at, Vector3.new(-20, 3, 0), Vector3.new(0, 43, 0))
	run("island_partial", at, Vector3.new(-20, 3, 0), Vector3.new(0, 43, 0), {
		PathSettings = { SupportPartialPath = true },
	})
	part({ Size = Vector3.new(6, 6, 6), Position = at + Vector3.new(-10, 3, 20) })
	run("goal_inside", at, Vector3.new(-20, 3, 0), Vector3.new(-10, 3, 20))

	-- 6. A Neon strip across the way, with and without a cost.
	at = course(6)
	floor(at)
	part({
		Size = Vector3.new(6, 1, 40),
		Position = at + Vector3.new(0, -0.45, -10),
		Material = Enum.Material.Neon,
	})
	run("neon_free", at, Vector3.new(-15, 3, -10), Vector3.new(15, 3, -10))
	run("neon_cost", at, Vector3.new(-15, 3, -10), Vector3.new(15, 3, -10), { Costs = { Neon = math.huge } })
	run("neon_cost10", at, Vector3.new(-15, 3, -10), Vector3.new(15, 3, -10), { Costs = { Neon = 10 } })
	-- A labelled region with a modifier.
	local zone = part({
		Size = Vector3.new(6, 6, 40),
		Position = at + Vector3.new(0, 3, 15),
		CanCollide = false,
		Transparency = 1,
	})
	local modifier = Instance.new("PathfindingModifier")
	modifier.Label = "Mud"
	modifier.Parent = zone
	run("label_free", at, Vector3.new(-15, 3, 15), Vector3.new(15, 3, 15))
	run("label_cost", at, Vector3.new(-15, 3, 15), Vector3.new(15, 3, 15), { Costs = { Mud = math.huge } })

	-- 7. A see-through wall with PassThrough, and a link over a pit.
	at = course(7)
	floor(at)
	local door = part({ Size = Vector3.new(2, 10, 60), Position = at + Vector3.new(0, 5, 0) })
	run("closed", at, Vector3.new(-15, 3, 0), Vector3.new(15, 3, 0))
	local pass = Instance.new("PathfindingModifier")
	pass.PassThrough = true
	pass.Parent = door
	run("pass_through", at, Vector3.new(-15, 3, 0), Vector3.new(15, 3, 0))
	door:Destroy()

	at = course(8)
	part({ Size = Vector3.new(20, 1, 20), Position = at + Vector3.new(-20, -0.5, 0) })
	part({ Size = Vector3.new(20, 1, 20), Position = at + Vector3.new(20, -0.5, 0) })
	run("pit", at, Vector3.new(-25, 3, 0), Vector3.new(25, 3, 0))
	local a0 = Instance.new("Attachment")
	a0.Position = Vector3.new(-12, 0.5, 0)
	local a1 = Instance.new("Attachment")
	a1.Position = Vector3.new(12, 0.5, 0)
	local anchor = part({ Size = Vector3.new(1, 1, 1), Position = at + Vector3.new(0, -5, 0), CanCollide = false })
	a0.Parent = anchor
	a1.Parent = anchor
	a0.WorldPosition = at + Vector3.new(-12, 0, 0)
	a1.WorldPosition = at + Vector3.new(12, 0, 0)
	local link = Instance.new("PathfindingLink")
	link.Attachment0 = a0
	link.Attachment1 = a1
	link.Label = "Boat"
	link.Parent = anchor
	run("link", at, Vector3.new(-25, 3, 0), Vector3.new(25, 3, 0))

	-- 9. Blocked: a wall dropped across a computed path.
	at = course(9)
	floor(at)
	local path = run("blocked_before", at, Vector3.new(-20, 3, 0), Vector3.new(20, 3, 0))
	local blocked = {}
	path.Blocked:Connect(function(index)
		table.insert(blocked, index)
	end)
	part({ Size = Vector3.new(2, 10, 60), Position = at + Vector3.new(0, 5, 0) })
	local frames = 0
	while #blocked == 0 and frames < 300 do
		h.beat()
		frames += 1
	end
	result.blocked_fired = #blocked
	result.blocked_index = blocked[1] or -1
	result._blocked_frames = frames
	result.occlusion = err(function()
		return path:CheckOcclusionAsync(1)
	end)
	result.occlusion_value = (function()
		local ok, v = pcall(path.CheckOcclusionAsync, path, 1)
		return ok and v or -99
	end)()

	-- Errors and defaults.
	result.params_bad = err(function()
		PathfindingService:CreatePath({ AgentRadius = "x" })
	end)
	result.params_unknown = err(function()
		PathfindingService:CreatePath({ Bogus = 1 })
	end)
	result.compute_bad = err(function()
		PathfindingService:CreatePath():ComputeAsync(1, 2)
	end)
	local fresh = PathfindingService:CreatePath()
	result.fresh_status = fresh.Status.Name
	result.fresh_waypoints = #fresh:GetWaypoints()
	result.path_class = fresh.ClassName
	result.find_path = err(function()
		local p = PathfindingService:FindPathAsync(course(1) + Vector3.new(-20, 3, 0), course(1) + Vector3.new(20, 3, 0))
		return p.Status.Name
	end)
	local w = PathWaypoint.new(Vector3.new(1, 2, 3), Enum.PathWaypointAction.Jump, "L")
	result.waypoint_new = string.format("%s %s %s", tostring(w.Position), w.Action.Name, w.Label)
	for _, p in made do
		p:Destroy()
	end
	return result
end
