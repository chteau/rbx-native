-- The player's character: LoadCharacter's event order, what a character
-- holds, where it spawns (which SpawnLocation, how high above it),
-- respawning after death with RespawnTime 1, StarterCharacterScripts and a
-- StarterCharacter model. Needs one player in the game.
--
-- Fields about the avatar itself (its parts, the HumanoidDescription,
-- animation ids) start with `_`: they depend on who joined.
return function(h, origin)
	local Players = game:GetService("Players")
	local StarterPlayer = game:GetService("StarterPlayer")
	local player = Players:GetPlayers()[1] or Players.PlayerAdded:Wait()
	if not player.Character then
		player.CharacterAdded:Wait()
	end
	local clock = h.clock()
	local result = {}
	local events = {}
	local mark = 0
	local function log(text)
		table.insert(events, string.format("%s@%d", text, clock.now() - mark))
	end
	player.CharacterRemoving:Connect(function(character)
		log("removing:" .. (character.Parent and "in" or "out"))
	end)
	player.CharacterAdded:Connect(function(character)
		log(
			string.format(
				"added:%s:%s:%s",
				character.Parent and character.Parent.Name or "nil",
				character:FindFirstChildOfClass("Humanoid") and "hum" or "nohum",
				character:FindFirstChild("HumanoidRootPart") and "root" or "noroot"
			)
		)
	end)
	player.CharacterAppearanceLoaded:Connect(function()
		log("appearance")
	end)

	local spawns = {}
	for _, d in workspace:GetDescendants() do
		if d:IsA("SpawnLocation") then
			spawns[d] = d.Enabled
			d.Enabled = false
		end
	end
	local floor = h.floor(origin)
	local function spawnAt(offset, props)
		local s = Instance.new("SpawnLocation")
		s.Anchored = true
		s.Size = Vector3.new(6, 1, 6)
		s.Position = origin + offset + Vector3.new(0, 0.5, 0)
		for key, value in props do
			s[key] = value
		end
		s.Parent = workspace
		return s
	end
	local a = spawnAt(Vector3.new(0, 0, 0), { Neutral = true })
	local function settle()
		for _ = 1, 30 do
			clock.tick()
		end
	end
	local function where(character)
		local root = character:WaitForChild("HumanoidRootPart")
		return h.vec(root.Position - origin, 1)
	end

	-- LoadCharacter: events, what the character holds, where it lands.
	local old = player.Character
	mark = clock.now()
	player:LoadCharacter()
	log("returned")
	for _ = 1, 60 do
		clock.tick()
	end
	result.load = table.concat(events, " ")
	table.clear(events)
	result.old_parent = old.Parent and old.Parent.Name or "nil"
	local character = player.Character
	local humanoid = character:FindFirstChildOfClass("Humanoid")
	result.spawn_a = where(character)
	result.yaw = h.yaw(character.HumanoidRootPart.CFrame)
	result.rig = humanoid.RigType.Name
	local scripts = {}
	for _, child in character:GetChildren() do
		if child:IsA("LuaSourceContainer") then
			table.insert(scripts, child.Name .. ":" .. child.ClassName)
		end
	end
	table.sort(scripts)
	result.scripts = table.concat(scripts, ",")
	result.primary = character.PrimaryPart and character.PrimaryPart.Name or "nil"
	result.animator = humanoid:FindFirstChildOfClass("Animator") and "yes" or "no"
	result.state = humanoid:GetState().Name
	result._hipheight = h.round(humanoid.HipHeight, 3)
	local parts = {}
	for _, child in character:GetChildren() do
		if child:IsA("BasePart") then
			table.insert(parts, child)
		end
	end
	result._parts = h.names(parts)
	local animate = character:FindFirstChild("Animate")
	if animate then
		local ids = {}
		for _, value in animate:GetChildren() do
			local anim = value:FindFirstChildOfClass("Animation")
			if anim then
				table.insert(ids, value.Name .. "=" .. anim.AnimationId)
			end
		end
		table.sort(ids)
		result._animate_ids = table.concat(ids, " ")
	end
	local ok, desc = pcall(function()
		return Players:GetHumanoidDescriptionFromUserId(player.UserId)
	end)
	if ok then
		local fields = {}
		for _, key in
			{
				"Head",
				"Torso",
				"LeftArm",
				"RightArm",
				"LeftLeg",
				"RightLeg",
				"Face",
				"Shirt",
				"Pants",
				"GraphicTShirt",
				"HeightScale",
				"WidthScale",
				"DepthScale",
				"HeadScale",
				"BodyTypeScale",
				"ProportionScale",
				"IdleAnimation",
				"WalkAnimation",
				"RunAnimation",
				"JumpAnimation",
				"FallAnimation",
				"ClimbAnimation",
				"SwimAnimation",
			}
		do
			table.insert(fields, key .. "=" .. tostring(desc[key]))
		end
		for _, key in { "HeadColor", "TorsoColor", "LeftArmColor", "RightArmColor", "LeftLegColor", "RightLegColor" } do
			table.insert(fields, key .. "=" .. desc[key]:ToHex())
		end
		local accessories = {}
		for _, acc in desc:GetAccessories(true) do
			table.insert(accessories, acc.AccessoryType.Name .. ":" .. acc.AssetId)
		end
		table.sort(accessories)
		table.insert(fields, "accessories=" .. table.concat(accessories, ","))
		result._description = table.concat(fields, " ")
	else
		result._description = "error: " .. tostring(desc)
	end

	-- What the client's own animation script plays, as the server sees it.
	local function tracks()
		local list = {}
		local animator = humanoid:FindFirstChildOfClass("Animator")
		for _, track in animator and animator:GetPlayingAnimationTracks() or {} do
			table.insert(
				list,
				string.format(
					"%s:%s:%s:%s",
					track.Animation and track.Animation.AnimationId or "?",
					h.round(track.Speed, 2),
					h.round(track.WeightTarget, 2),
					track.Priority.Name
				)
			)
		end
		table.sort(list)
		return table.concat(list, " ")
	end
	result._idle_tracks = tracks()
	humanoid:MoveTo(character.HumanoidRootPart.Position + Vector3.new(0, 0, 40))
	for _ = 1, 30 do
		clock.tick()
	end
	result._walk16_tracks = tracks()
	humanoid.WalkSpeed = 8
	humanoid:MoveTo(character.HumanoidRootPart.Position + Vector3.new(0, 0, -40))
	for _ = 1, 30 do
		clock.tick()
	end
	result._walk8_tracks = tracks()

	-- Death and respawn with RespawnTime 1, a StarterCharacterScripts probe.
	local respawn = Players.RespawnTime
	Players.RespawnTime = 1
	local probe = Instance.new("Script")
	probe.Name = "CharProbe"
	probe.Disabled = true
	probe.Parent = StarterPlayer:FindFirstChildOfClass("StarterCharacterScripts")
	mark = clock.now()
	character:FindFirstChildOfClass("Humanoid").Health = 0
	log("killed")
	local respawned = nil
	local conn = player.CharacterAdded:Connect(function(c)
		respawned = c
	end)
	while not respawned and clock.now() - mark < 600 do
		clock.tick()
	end
	conn:Disconnect()
	for _ = 1, 10 do
		clock.tick()
	end
	result.respawn = table.concat(events, " ")
	table.clear(events)
	result.probe = respawned and respawned:FindFirstChild("CharProbe") and respawned.CharProbe.ClassName or "missing"
	probe:Destroy()
	Players.RespawnTime = respawn

	-- Which spawn: A off, B for a team the player isn't on, C neutral.
	a.Enabled = false
	local b = spawnAt(Vector3.new(20, 0, 0), { Neutral = false, TeamColor = BrickColor.new("Bright red") })
	local c = spawnAt(Vector3.new(-20, 0, 0), { Neutral = true })
	player:LoadCharacter()
	settle()
	result.spawn_bc = where(player.Character)
	c.Enabled = false
	player:LoadCharacter()
	settle()
	-- No spawn will have it: where it starts over the origin. (How far it
	-- has fallen since depends on when the client took the physics over.)
	local function flat(character)
		local position = character:WaitForChild("HumanoidRootPart").Position
		return h.vec(Vector3.new(position.X, 0, position.Z), 0)
	end
	result.spawn_b_only = flat(player.Character)
	b.Enabled = false
	player:LoadCharacter()
	settle()
	local root = player.Character:WaitForChild("HumanoidRootPart")
	result.spawn_none = flat(player.Character)
	result._spawn_none_y = h.round(root.Position.Y, 1)

	-- A StarterCharacter model replaces the avatar.
	local starter = h.character(Vector3.zero, "R15", nil, true)
	starter.Name = "StarterCharacter"
	starter.Parent = StarterPlayer
	a.Enabled = true
	player:LoadCharacter()
	settle()
	local custom = player.Character
	local customParts = {}
	for _, child in custom:GetChildren() do
		if child:IsA("BasePart") then
			table.insert(customParts, child)
		end
	end
	result.starter_parts = h.names(customParts)
	result.starter_name = custom.Name == player.Name
	result.starter_scripts = (custom:FindFirstChild("Animate") and "Animate" or "-")
		.. ","
		.. (custom:FindFirstChild("Health") and "Health" or "-")
	result.starter_at = where(custom)
	result.starter_hipheight = h.round(custom:FindFirstChildOfClass("Humanoid").HipHeight, 3)
	starter:Destroy()

	for s, enabled in spawns do
		s.Enabled = enabled
	end
	for _, s in { a, b, c } do
		s:Destroy()
	end
	player:LoadCharacter()
	floor:Destroy()
	return result
end
