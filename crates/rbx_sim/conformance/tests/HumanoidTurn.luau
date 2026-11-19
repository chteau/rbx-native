-- AutoRotate: an R15 facing -Z starts moving along +X, then along -X; its
-- facing every beat. A second one with AutoRotate off.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(160, 2, 160))
	local model, humanoid, root = h.character(origin + Vector3.new(-30, 0, -10), "R15")
	local fixed, fixedHumanoid, fixedRoot =
		h.character(origin + Vector3.new(-30, 0, 10), "R15", { AutoRotate = false })
	h.beats(30)
	local result = {}
	humanoid:Move(Vector3.new(1, 0, 0))
	fixedHumanoid:Move(Vector3.new(1, 0, 0))
	local yaw = {}
	for i = 1, 30 do
		h.beat()
		local a = h.yaw(root.CFrame)
		table.insert(yaw, a)
		if i == 1 or i == 2 or i == 3 or i == 5 or i == 10 or i == 30 then
			result["yaw_" .. i] = a
		end
		if a >= 89.5 and result.turned_beat == nil then
			result.turned_beat = i
		end
	end
	result.fixed_yaw = h.yaw(fixedRoot.CFrame)
	result.fixed_dx = h.round(fixedRoot.Position.X - (origin.X - 30), 1)
	humanoid:Move(Vector3.new(-1, 0, 0))
	local back = {}
	for i = 1, 40 do
		h.beat()
		local a = h.yaw(root.CFrame)
		table.insert(back, a)
		if i == 1 or i == 3 or i == 5 or i == 10 or i == 40 then
			result["back_" .. i] = a
		end
		if math.abs(a) >= 179.5 and result.back_beat == nil then
			result.back_beat = i
		end
	end
	result._yaw = yaw
	result._back = back
	model:Destroy()
	fixed:Destroy()
	floor:Destroy()
	return result
end
