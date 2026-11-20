-- A character dropped into a lake 24 deep: its states, where it floats,
-- its swim speed under Move, a jump at the surface, and swimming onto a
-- shelf 3 under the surface (back on its feet). R15 and R6.
return function(h, origin)
	local terrain = workspace.Terrain
	local function at(x, y, z)
		return origin + Vector3.new(x, y, z)
	end
	-- Surface at origin.Y; the shelf at X 16..32 has its top 3 below it.
	terrain:FillBlock(CFrame.new(at(0, -12, 0)), Vector3.new(64, 24, 64), Enum.Material.Water)
	terrain:FillBlock(CFrame.new(at(0, -28, 0)), Vector3.new(64, 8, 64), Enum.Material.Rock)
	terrain:FillBlock(CFrame.new(at(24, -14, 0)), Vector3.new(16, 22, 64), Enum.Material.Rock)
	-- New terrain takes a few frames to collide.
	h.beats(30)
	local result = {}
	for _, rig in { { "r15", "R15", -8 }, { "r6", "R6", 8 } } do
		local name, kind, z = rig[1], rig[2], rig[3]
		local clock = h.clock()
		local model, humanoid, root = h.character(at(-8, 2, z), kind)
		local log = h.states(humanoid, clock)
		local swimSpeed = -1
		humanoid.Swimming:Connect(function(speed)
			swimSpeed = speed
		end)
		local trace = {}
		for i = 1, 120 do
			clock.tick()
			if i % 5 == 0 then
				table.insert(trace, h.round(root.Position.Y - origin.Y, 2))
			end
		end
		result[name .. "_float"] = h.round(root.Position.Y - origin.Y, 2)
		result[name .. "_state"] = humanoid:GetState().Name
		result[name .. "_idle_speed"] = h.round(swimSpeed, 2)
		result["_" .. name .. "_sink"] = trace
		-- Swim along -X (away from the shelf) for a second.
		local x0 = root.Position.X
		humanoid:Move(Vector3.new(-1, 0, 0))
		for i = 1, 60 do
			clock.tick()
			if i == 10 or i == 30 or i == 60 then
				result[name .. "_vx" .. i] = h.round(root.AssemblyLinearVelocity.X, 2)
			end
		end
		result[name .. "_dx"] = h.round(root.Position.X - x0, 2)
		result[name .. "_swim_speed"] = h.round(swimSpeed, 2)
		result[name .. "_dy_moving"] = h.round(root.Position.Y - origin.Y, 2)
		humanoid:Move(Vector3.zero)
		for _ = 1, 30 do
			clock.tick()
		end
		local first = #log
		local y0 = root.Position.Y
		humanoid.Jump = true
		local peak = 0
		for _ = 1, 45 do
			clock.tick()
			peak = math.max(peak, root.Position.Y - y0)
		end
		result[name .. "_jump_rise"] = h.round(peak, 2)
		local jumpLog = {}
		for i = first + 1, #log do
			table.insert(jumpLog, (string.gsub(log[i], "@%d+", "")))
		end
		result[name .. "_jump_states"] = table.concat(jumpLog, " ")
		humanoid.Jump = false
		for _ = 1, 60 do
			clock.tick()
		end
		-- Onto the shelf.
		humanoid:Move(Vector3.new(1, 0, 0))
		for _ = 1, 210 do
			clock.tick()
		end
		humanoid:Move(Vector3.zero)
		for _ = 1, 30 do
			clock.tick()
		end
		result[name .. "_shelf_state"] = humanoid:GetState().Name
		result[name .. "_shelf_y"] = h.round(root.Position.Y - origin.Y, 2)
		result[name .. "_shelf_floor"] = humanoid.FloorMaterial.Name
		result["_" .. name .. "_states"] = table.concat(log, " ")
		local seen = {}
		for _, entry in log do
			table.insert(seen, (string.gsub(entry, "@%d+", "")))
		end
		result[name .. "_states"] = table.concat(seen, " ")
		model:Destroy()
	end
	terrain:Clear()
	return result
end
