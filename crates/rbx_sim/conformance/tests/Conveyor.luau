-- An anchored part with AssemblyLinearVelocity set is a conveyor: a block
-- and a character standing on one, how fast they move and how far they go
-- in a second; the belt itself doesn't move.
return function(h, origin)
	local belt = h.part({
		Name = "Belt",
		Anchored = true,
		Size = Vector3.new(80, 1, 12),
		Position = origin + Vector3.new(0, -0.5, 0),
	})
	belt.AssemblyLinearVelocity = Vector3.new(10, 0, 0)
	local block = h.part({ Name = "Block", Size = Vector3.new(2, 2, 2), Position = origin + Vector3.new(-30, 1, -3) })
	local model, _, root = h.character(origin + Vector3.new(-30, 0, 3), "R15")
	h.beats(5)
	local b0, r0, belt0 = block.Position, root.Position, belt.Position
	local result = { _block = {}, _root = {} }
	for i = 1, 60 do
		h.beat()
		table.insert(result._block, h.round(block.AssemblyLinearVelocity.X, 2))
		table.insert(result._root, h.round(root.AssemblyLinearVelocity.X, 2))
		if i == 10 or i == 60 then
			result["block_vx" .. i] = h.round(block.AssemblyLinearVelocity.X, 2)
			result["root_vx" .. i] = h.round(root.AssemblyLinearVelocity.X, 2)
		end
	end
	result.block_dx = h.round(block.Position.X - b0.X, 2)
	result.root_dx = h.round(root.Position.X - r0.X, 2)
	result.belt_moved = h.round((belt.Position - belt0).Magnitude, 3)
	result.belt_v = h.vec(belt.AssemblyLinearVelocity, 2)
	model:Destroy()
	block:Destroy()
	belt:Destroy()
	return result
end
