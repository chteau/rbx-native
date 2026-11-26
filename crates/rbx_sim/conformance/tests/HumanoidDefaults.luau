-- What a fresh Humanoid and the player settings start as: properties,
-- which states are enabled, the state before it's in a world.
return function(h, origin)
	local humanoid = Instance.new("Humanoid")
	local result = {}
	for _, key in
		{
			"WalkSpeed",
			"JumpPower",
			"JumpHeight",
			"UseJumpPower",
			"HipHeight",
			"MaxSlopeAngle",
			"AutoRotate",
			"AutoJumpEnabled",
			"Health",
			"MaxHealth",
			"BreakJointsOnDeath",
			"RequiresNeck",
			"PlatformStand",
			"Sit",
			"EvaluateStateMachine",
		}
	do
		local value = humanoid[key]
		result[key] = if type(value) == "number" then h.round(value, 3) else value
	end
	result.RigType = humanoid.RigType.Name
	result.state = humanoid:GetState().Name
	local off = {}
	for _, state in Enum.HumanoidStateType:GetEnumItems() do
		if not humanoid:GetStateEnabled(state) then
			table.insert(off, state.Name)
		end
	end
	table.sort(off)
	result.disabled_states = table.concat(off, ",")
	local player = game:GetService("StarterPlayer")
	for _, key in
		{
			"CharacterWalkSpeed",
			"CharacterJumpPower",
			"CharacterJumpHeight",
			"CharacterUseJumpPower",
			"CharacterMaxSlopeAngle",
			"AutoJumpEnabled",
			"LoadCharacterAppearance",
		}
	do
		local value = player[key]
		result["starter_" .. key] = if type(value) == "number" then h.round(value, 3) else value
	end
	local players = game:GetService("Players")
	result.respawn_time = players.RespawnTime
	result.autoloads = players.CharacterAutoLoads
	humanoid:Destroy()
	return result
end
