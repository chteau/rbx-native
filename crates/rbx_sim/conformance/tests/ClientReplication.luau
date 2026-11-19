-- What a client's own doings show on the server: a Sound it plays and stops
-- (and a property it writes), and the velocity of its character walking, as
-- the server sees it next to what the client sees. Needs a player.
return function(h, origin)
	local SoundService = game:GetService("SoundService")
	local player = h.player()
	local result = {}
	result.respect_filtering = tostring(SoundService.RespectFilteringEnabled)
	local sound = Instance.new("Sound")
	sound.Name = "ConfSound"
	sound.Looped = true
	sound.Volume = 0.5
	sound.Parent = workspace
	h.beats(10)
	h.client("ClientReplication", "play", sound)
	h.beats(10)
	result.played = tostring(sound.Playing) .. ":" .. tostring(sound.IsPlaying)
	result.volume = h.round(sound.Volume, 2)
	h.client("ClientReplication", "stop", sound)
	h.beats(10)
	result.stopped = tostring(sound.Playing)
	sound:Destroy()

	local character = player.Character or player.CharacterAdded:Wait()
	local humanoid = character:WaitForChild("Humanoid")
	local root = character:WaitForChild("HumanoidRootPart")
	h.beats(30)
	local floor = h.floor(origin)
	character:PivotTo(CFrame.new(origin + Vector3.new(0, 3, 0)))
	h.beats(60)
	humanoid:MoveTo(origin + Vector3.new(0, 0, -60))
	h.beats(45)
	result.server_speed = h.round(root.AssemblyLinearVelocity.Magnitude, 1)
	local seen = h.client("ClientReplication", "speed")
	result.client_speed = seen.speed
	h.beats(240)
	result.server_stopped_speed = h.round(root.AssemblyLinearVelocity.Magnitude, 1)
	floor:Destroy()
	player:LoadCharacter()
	h.beats(30)
	return result
end
