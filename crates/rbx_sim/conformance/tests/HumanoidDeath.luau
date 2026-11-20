-- Death on an R15 built by the harness: with BreakJointsOnDeath (the
-- default) the joints go and the body falls apart; without it the body
-- stays in one piece and falls over. When Died fires, how many Motor6Ds
-- are left, which parts still collide, the state, and where the head and a
-- hand are 15 and 60 beats later. GetCanCollideOnDeath on each part.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(80, 2, 80))
	local result = {}
	local function run(prefix, feet, breaks)
		local model, humanoid, root = h.character(feet, "R15", { BreakJointsOnDeath = breaks })
		h.beats(30)
		local clock = h.clock()
		local died
		humanoid.Died:Connect(function()
			died = clock.now()
		end)
		local states = h.states(humanoid, clock)
		local head, hand = model.Head, model.LeftHand
		local y0, hand0 = head.Position.Y, hand.Position
		humanoid.Health = 0
		local motors = function()
			local n = 0
			for _, d in model:GetDescendants() do
				if d:IsA("Motor6D") then
					n += 1
				end
			end
			return n
		end
		result[prefix .. "_motors_now"] = motors()
		for i = 1, 60 do
			clock.tick()
			if i == 1 then
				result[prefix .. "_motors_1"] = motors()
			end
			if i == 15 or i == 60 then
				result[prefix .. "_head_dy_" .. i] = h.round(head.Position.Y - y0, 2)
				result[prefix .. "_hand_moved_" .. i] = h.round((hand.Position - hand0).Magnitude, 2)
			end
		end
		result[prefix .. "_died_beat"] = died or -1
		result[prefix .. "_state"] = humanoid:GetState().Name
		result[prefix .. "_states"] = table.concat(states, " ")
		result[prefix .. "_health"] = humanoid.Health
		local collide = {}
		for _, name in { "Head", "UpperTorso", "LeftHand", "HumanoidRootPart" } do
			local part = model:FindFirstChild(name)
			if part then
				table.insert(collide, name .. "=" .. tostring(part.CanCollide))
			end
		end
		result[prefix .. "_collide"] = table.concat(collide, " ")
		local ok, value = pcall(function()
			return humanoid:GetCanCollideOnDeath(head, false)
		end)
		result[prefix .. "_can_collide_head"] = if ok then tostring(value) else "err"
		model:Destroy()
	end
	run("breaks", origin + Vector3.new(-15, 0, 0), true)
	run("keeps", origin + Vector3.new(15, 0, 0), false)
	floor:Destroy()
	return result
end
