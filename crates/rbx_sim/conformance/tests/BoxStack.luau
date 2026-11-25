-- Five 2x2x2 blocks stacked touching on a floor: after two seconds, how
-- far the top one sank and drifted and how fast anything still moves.
return function(h, origin)
	local floor = h.part({ Name = "Floor", Anchored = true, Size = Vector3.new(20, 1, 20), Position = origin })
	local blocks = {}
	for i = 1, 5 do
		blocks[i] = h.part({
			Name = "Block" .. i,
			Size = Vector3.new(2, 2, 2),
			Position = origin + Vector3.new(0, 1.5 + (i - 1) * 2, 0),
		})
	end
	local top = blocks[5]
	local start = top.Position
	local trace = {}
	for _ = 1, 120 do
		h.beat()
		table.insert(trace, h.round(top.Position.Y - start.Y, 3))
	end
	local fastest = 0
	for _, block in blocks do
		fastest = math.max(fastest, block.AssemblyLinearVelocity.Magnitude)
	end
	local r = {
		top_dy = h.round(top.Position.Y - start.Y, 3),
		top_drift = h.round(Vector3.new(top.Position.X - start.X, 0, top.Position.Z - start.Z).Magnitude, 3),
		fastest = h.round(fastest, 3),
		bottom_dy = h.round(blocks[1].Position.Y - origin.Y - 1.5, 3),
		_top = trace,
	}
	h.clear(blocks)
	floor:Destroy()
	return r
end
