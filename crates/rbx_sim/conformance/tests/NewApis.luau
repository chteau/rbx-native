-- Newer engine API a game may lean on, as a live server answers it:
-- RunService:BindToSimulationParallel/BindToAnimationParallel called from a
-- plain Script, AvatarCollisionRules and its defaults, Humanoid
-- GetCanCollideOnDeath, AudioStreamReader/Writer, DataModel.ServerRegion
-- and Workspace.NextGenerationPhysicsSolver.
return function(h, origin)
	local RunService = game:GetService("RunService")
	local result = {}
	result.bind_sim = h.try(function()
		return RunService:BindToSimulationParallel(function() end)
	end)
	result.bind_anim = h.try(function()
		return RunService:BindToAnimationParallel(function() end)
	end)
	result.rules_new = h.try(function()
		return typeof(Instance.new("AvatarCollisionRules"))
	end)
	local ok, rules = pcall(Instance.new, "AvatarCollisionRules")
	if ok then
		h.props(result, "rules", rules, {
			"BodyPartsCollidableOnDeath",
			"CollisionMode",
			"HitAndTouchDetectionMode",
			"HitboxSize",
			"LegacyCollisionMode",
			"SingleColliderSize",
			"TouchAndQueryDetectionMode",
		})
	end
	local found = {}
	for _, d in game:GetDescendants() do
		local isRules = pcall(function()
			assert(d:IsA("AvatarCollisionRules") or d:IsA("AvatarRules"))
		end)
		if isRules then
			table.insert(found, d.ClassName .. "@" .. d.Parent.ClassName)
		end
	end
	table.sort(found)
	result.rules_in_game = table.concat(found, " ")

	local model, humanoid = h.character(origin, "R15")
	h.beats(5)
	for _, name in { "Head", "UpperTorso", "LeftHand", "HumanoidRootPart" } do
		result["collide_on_death." .. name] = h.try(function()
			return humanoid:GetCanCollideOnDeath(model[name], false)
		end)
		result["collide_on_death_true." .. name] = h.try(function()
			return humanoid:GetCanCollideOnDeath(model[name], true)
		end)
	end
	result.uses_avatar_collider = h.try(function()
		return humanoid.UsesAvatarCollider
	end)
	model:Destroy()

	for _, class in { "AudioStreamReader", "AudioStreamWriter" } do
		local made, stream = pcall(Instance.new, class)
		result[class .. ".new"] = if made then "ok" else "err"
		if made then
			h.props(result, class, stream, { "Layout", "SampleFormat", "SampleRate", "WindowSize" })
			result[class .. ".pins"] = h.try(function()
				return #stream:GetInputPins() .. "/" .. #stream:GetOutputPins()
			end)
		end
	end
	result.reader_read = h.try(function()
		local r = Instance.new("AudioStreamReader")
		return typeof(r:Read())
	end)
	result.writer_space = h.try(function()
		return Instance.new("AudioStreamWriter"):Space()
	end)

	result.region = h.try(function()
		return typeof(game.ServerRegion) .. ":" .. tostring(game.ServerRegion ~= "")
	end)
	result.next_gen_solver = h.try(function()
		return workspace.NextGenerationPhysicsSolver
	end)
	return result
end
