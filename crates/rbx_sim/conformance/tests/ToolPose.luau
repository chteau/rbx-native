-- The arm pose the default Animate gives a character holding a tool: the
-- tracks playing on the client and its right shoulder's angles, without
-- and with a tool equipped, standing still. Needs a player.
return function(h)
	local player = h.player()
	player:LoadCharacter()
	local character = player.Character or player.CharacterAdded:Wait()
	local humanoid = character:WaitForChild("Humanoid")
	h.beats(90)
	local result = {}
	result.rig = humanoid.RigType.Name
	local before = h.client("ToolPose")
	result.idle_tracks = before.tracks
	result.idle_shoulder = before.shoulder
	local tool = Instance.new("Tool")
	tool.Name = "ConfPoseTool"
	local handle = Instance.new("Part")
	handle.Name = "Handle"
	handle.Size = Vector3.new(1, 1, 3)
	handle.Parent = tool
	tool.Parent = player.Backpack
	humanoid:EquipTool(tool)
	h.beats(60)
	local held = h.client("ToolPose")
	result.held_tracks = held.tracks
	result.held_shoulder = held.shoulder
	humanoid:UnequipTools()
	h.beats(60)
	local after = h.client("ToolPose")
	result.after_tracks = after.tracks
	tool:Destroy()
	player:LoadCharacter()
	h.beats(30)
	return result
end
