-- The state machine: a fresh character's first states, a jump with Jumping
-- disabled, ChangeState to Freefall on the ground, PlatformStand on and
-- off, ChangeState to Physics then GettingUp, FallingDown, Ragdoll.
return function(h, origin)
	local floor = h.floor(origin)
	local clock = h.clock()
	local model, humanoid, root = h.character(origin + Vector3.new(0, 0, 0), "R15")
	local result = {}
	result.fresh = humanoid:GetState().Name
	local log = h.states(humanoid, clock)
	for _ = 1, 30 do
		clock.tick()
	end
	result.settled = humanoid:GetState().Name
	local function phase(name, beats, act)
		local first = #log
		local start = clock.now()
		act()
		local y0 = root.Position.Y
		local peak = 0
		for _ = 1, beats do
			clock.tick()
			peak = math.max(peak, root.Position.Y - y0)
		end
		local entries = {}
		for i = first + 1, #log do
			local old, new, beat = string.match(log[i], "(%w+)>(%w+)@(%d+)")
			table.insert(entries, string.format("%s>%s@%d", old, new, tonumber(beat) - start))
		end
		result[name] = table.concat(entries, " ")
		result[name .. "_end"] = humanoid:GetState().Name
		return peak
	end
	result.nojump_rise = h.round(
		phase("nojump", 30, function()
			humanoid:SetStateEnabled(Enum.HumanoidStateType.Jumping, false)
			humanoid.Jump = true
		end),
		2
	)
	result.nojump_enabled = humanoid:GetStateEnabled(Enum.HumanoidStateType.Jumping)
	humanoid:SetStateEnabled(Enum.HumanoidStateType.Jumping, true)
	result.nojump_flag = humanoid.Jump
	humanoid.Jump = false
	phase("freefall", 30, function()
		humanoid:ChangeState(Enum.HumanoidStateType.Freefall)
	end)
	phase("platform", 20, function()
		humanoid.PlatformStand = true
	end)
	phase("unplatform", 30, function()
		humanoid.PlatformStand = false
	end)
	phase("physics", 30, function()
		humanoid:ChangeState(Enum.HumanoidStateType.Physics)
	end)
	phase("gettingup", 60, function()
		humanoid:ChangeState(Enum.HumanoidStateType.GettingUp)
	end)
	phase("fallingdown", 120, function()
		humanoid:ChangeState(Enum.HumanoidStateType.FallingDown)
	end)
	phase("ragdoll", 120, function()
		humanoid:ChangeState(Enum.HumanoidStateType.Ragdoll)
	end)
	result.upright = h.round(root.CFrame.UpVector.Y, 2)
	model:Destroy()
	floor:Destroy()
	return result
end
