-- HipHeight changed under a standing R15 (2 to 4, then 1): the root's
-- height every beat. A PlatformStanding and a Ragdoll character pushed
-- sideways at the head: whether they tip over.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(120, 2, 120))
	local model, humanoid, root = h.character(origin + Vector3.new(-20, 0, 0), "R15")
	local stand, standHumanoid, standRoot = h.character(origin + Vector3.new(0, 0, 0), "R15")
	local rag, ragHumanoid, ragRoot = h.character(origin + Vector3.new(20, 0, 0), "R15")
	h.beats(30)
	local y0 = root.Position.Y
	local result = { _up = {}, _down = {} }
	humanoid.HipHeight = 4
	for _ = 1, 30 do
		h.beat()
		table.insert(result._up, h.round(root.Position.Y - y0, 3))
	end
	result.up_dy = h.round(root.Position.Y - y0, 3)
	humanoid.HipHeight = 1
	for _ = 1, 30 do
		h.beat()
		table.insert(result._down, h.round(root.Position.Y - y0, 3))
	end
	result.down_dy = h.round(root.Position.Y - y0, 3)
	standHumanoid.PlatformStand = true
	ragHumanoid:ChangeState(Enum.HumanoidStateType.Ragdoll)
	h.beat()
	for _, r in { stand, rag } do
		r.Head:ApplyImpulse(Vector3.new(r.Head.AssemblyMass * 20, 0, 0))
	end
	h.beats(60)
	result.stand_state = standHumanoid:GetState().Name
	result.stand_up = h.round(standRoot.CFrame.UpVector.Y, 2)
	result.rag_state = ragHumanoid:GetState().Name
	result.rag_up = h.round(ragRoot.CFrame.UpVector.Y, 2)
	model:Destroy()
	stand:Destroy()
	rag:Destroy()
	floor:Destroy()
	return result
end
