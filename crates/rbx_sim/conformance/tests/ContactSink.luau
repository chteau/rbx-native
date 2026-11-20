-- How far resting blocks sink into the floor with the load on them: single
-- 2 stud blocks of density 0.7, 7 and 70, a 4 stud block, stacks of two
-- and three, a dense block on a light one; each block's drop after two
-- seconds.
return function(h, origin)
	local floor = h.floor(origin, Vector3.new(80, 2, 20))
	local function block(x, level, density, size)
		local s = size or 2
		return h.part({
			Size = Vector3.new(s, s, s),
			Position = origin + Vector3.new(x, s / 2 + level, 0),
			CustomPhysicalProperties = PhysicalProperties.new(density, 0.3, 0.5),
		})
	end
	local cases = {
		d07 = { block(-30, 0, 0.7) },
		d7 = { block(-20, 0, 7) },
		d70 = { block(-10, 0, 70) },
		big = { block(0, 0, 0.7, 4) },
		two = { block(10, 0, 0.7), block(10, 2, 0.7) },
		three = { block(20, 0, 0.7), block(20, 2, 0.7), block(20, 4, 0.7) },
		heavytop = { block(30, 0, 0.7), block(30, 2, 7) },
	}
	local start = {}
	for name, blocks in cases do
		start[name] = {}
		for i, b in blocks do
			start[name][i] = b.Position.Y
		end
	end
	local result = { _d70 = {} }
	for _ = 1, 120 do
		h.beat()
		table.insert(result._d70, h.round(cases.d70[1].Position.Y - start.d70[1], 4))
	end
	for name, blocks in cases do
		for i, b in blocks do
			result[name .. "_" .. i] = h.round(b.Position.Y - start[name][i], 4)
		end
		h.clear(blocks)
	end
	floor:Destroy()
	return result
end
