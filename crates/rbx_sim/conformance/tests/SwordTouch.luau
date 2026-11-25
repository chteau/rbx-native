-- A sword-like tool held by an R15 walking into an anchored target: the
-- beat Activated fires after Tool:Activate, the beat the handle first
-- touches the target, whether the target's own Touched names the handle,
-- and the root's distance walked by then. The character is built here, so
-- the server equips it.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(80, 2, 80))
	local model, humanoid, root = h.character(origin + Vector3.new(0, 0, 10), "R15")
	local tool = Instance.new("Tool")
	tool.Name = "Sword"
	local handle = Instance.new("Part")
	handle.Name = "Handle"
	handle.Size = Vector3.new(1, 0.8, 4)
	handle.CanCollide = false
	handle.Parent = tool
	tool.Parent = workspace
	h.beats(30)
	local result = {}
	local clock = h.clock()
	humanoid:EquipTool(tool)
	result.equipped = tool.Parent == model
	result.grip = model:FindFirstChild("RightGrip", true) ~= nil
	local activated
	tool.Activated:Connect(function()
		activated = clock.now()
	end)
	tool:Activate()
	result.activated_now = activated ~= nil
	clock.tick()
	result.activated_beat = activated or -1
	local target = h.part({
		Name = "Target",
		Size = Vector3.new(6, 6, 1),
		Position = origin + Vector3.new(0, 3, 0),
		Anchored = true,
	})
	local touched, by
	target.Touched:Connect(function(other)
		if not touched then
			touched = clock.now()
			by = other.Name
		end
	end)
	local z0 = root.Position.Z
	local start = clock.now()
	humanoid:Move(Vector3.new(0, 0, -1))
	for _ = 1, 90 do
		clock.tick()
		if touched then
			break
		end
	end
	result.touch_beats = if touched then touched - start else -1
	result.touch_by = by or "none"
	result.walked = h.round(z0 - root.Position.Z, 1)
	humanoid:Move(Vector3.zero)
	model:Destroy()
	target:Destroy()
	floor:Destroy()
	return result
end
