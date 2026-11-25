-- Health: TakeDamage, clamping to MaxHealth, a ForceField, death (Died,
-- HealthChanged and the Dead state in order, joints broken or not by
-- BreakJointsOnDeath), RequiresNeck when the neck goes.
return function(h, origin)
	local floor = h.floor(origin)
	local clock = h.clock()
	local model, humanoid = h.character(origin + Vector3.new(-20, 0, 0), "R15")
	local kept, keptHumanoid = h.character(origin + Vector3.new(-10, 0, 0), "R15", { BreakJointsOnDeath = false })
	local necked, neckedHumanoid = h.character(origin + Vector3.new(0, 0, 0), "R15")
	local loose, looseHumanoid = h.character(origin + Vector3.new(10, 0, 0), "R15", { RequiresNeck = false })
	local r6, r6Humanoid = h.character(origin + Vector3.new(20, 0, 0), "R6")
	for _ = 1, 30 do
		clock.tick()
	end
	local result = {}
	local events = {}
	local start = clock.now()
	local function log(text)
		table.insert(events, string.format("%s@%d", text, clock.now() - start))
	end
	humanoid.HealthChanged:Connect(function(health)
		log("health:" .. h.round(health, 2))
	end)
	humanoid.Died:Connect(function()
		log("died")
	end)
	humanoid.StateChanged:Connect(function(_, new)
		log("state:" .. new.Name)
	end)
	humanoid:TakeDamage(30)
	result.after_damage = humanoid.Health
	humanoid.Health = 200
	result.over_max = humanoid.Health
	humanoid.MaxHealth = 50
	result.max_lowered = humanoid.Health
	humanoid.MaxHealth = 100
	humanoid.Health = 100
	humanoid:TakeDamage(-10)
	result.negative_damage = humanoid.Health
	local field = Instance.new("ForceField")
	field.Parent = model
	humanoid:TakeDamage(30)
	result.with_forcefield = humanoid.Health
	field:Destroy()
	clock.tick()
	log("mark")
	humanoid:TakeDamage(1000)
	result.overkill = humanoid.Health
	local function motors(rig)
		local n = 0
		for _, d in rig:GetDescendants() do
			if d:IsA("Motor6D") then
				n += 1
			end
		end
		return n
	end
	keptHumanoid.Health = 0
	r6Humanoid.Health = 0
	necked:FindFirstChild("Neck", true):Destroy()
	loose:FindFirstChild("Neck", true):Destroy()
	for i = 1, 90 do
		clock.tick()
		if i == 1 or i == 60 then
			result["motors_" .. i] = motors(model)
			result["kept_motors_" .. i] = motors(kept)
			result["r6_motors_" .. i] = motors(r6)
			result["state_" .. i] = humanoid:GetState().Name
		end
	end
	result.dead_health = humanoid.Health
	humanoid.Health = 50
	result.revive_health = humanoid.Health
	clock.tick()
	result.revive_state = humanoid:GetState().Name
	result.parent = model.Parent and model.Parent.Name or "nil"
	result.root_parent = model:FindFirstChild("HumanoidRootPart") and "kept" or "gone"
	result.kept_state = keptHumanoid:GetState().Name
	result.necked_state = neckedHumanoid:GetState().Name
	result.necked_health = neckedHumanoid.Health
	result.loose_state = looseHumanoid:GetState().Name
	result.loose_health = looseHumanoid.Health
	result.events = table.concat(events, " ")
	for _, thing in { model, kept, necked, loose, r6, floor } do
		thing:Destroy()
	end
	return result
end
