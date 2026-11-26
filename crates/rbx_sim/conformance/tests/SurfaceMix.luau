-- How two surfaces' Friction and Elasticity combine with their weights:
-- blocks pushed at 20 stud/s along floors of other friction (how far they
-- slide), blocks dropped 10 studs on floors of other elasticity (how high
-- the first bounce gets).
return function(h, origin)
	local function props(friction, fw, elasticity, ew)
		return PhysicalProperties.new(0.7, friction, elasticity, fw, ew)
	end
	-- name, block friction, weight, floor friction, weight
	local slides = {
		{ "f_same", 0.3, 1, 0.3, 1 },
		{ "f_mix", 0.1, 1, 0.9, 1 },
		{ "f_w4", 0.1, 4, 0.9, 1 },
		{ "f_floorw4", 0.1, 1, 0.9, 4 },
		{ "f_w0", 0.1, 0, 0.9, 1 },
		{ "f_both0", 0.1, 0, 0.9, 0 },
	}
	-- name, block elasticity, weight, floor elasticity, weight
	local bounces = {
		{ "e_mix", 0.9, 1, 0.1, 1 },
		{ "e_w4", 0.9, 4, 0.1, 1 },
		{ "e_floorw4", 0.9, 1, 0.1, 4 },
		{ "e_w0", 0.9, 0, 0.1, 1 },
		{ "e_high", 1, 1, 1, 1 },
	}
	local parts, blocks, drops = {}, {}, {}
	for i, s in slides do
		local z = -60 + i * 8
		table.insert(parts, h.part({
			Anchored = true,
			Size = Vector3.new(200, 1, 6),
			Position = origin + Vector3.new(0, 0, z),
			CustomPhysicalProperties = props(s[4], s[5], 0.5, 1),
		}))
		local b = h.part({
			Size = Vector3.new(2, 2, 2),
			Position = origin + Vector3.new(-80, 1.5, z),
			CustomPhysicalProperties = props(s[2], s[3], 0.5, 1),
		})
		table.insert(parts, b)
		blocks[s[1]] = b
	end
	for i, s in bounces do
		local z = 10 + i * 8
		table.insert(parts, h.part({
			Anchored = true,
			Size = Vector3.new(6, 1, 6),
			Position = origin + Vector3.new(0, 0, z),
			CustomPhysicalProperties = props(0.3, 1, s[4], s[5]),
		}))
		local b = h.part({
			Anchored = true,
			Size = Vector3.new(2, 2, 2),
			Position = origin + Vector3.new(0, 11.5, z),
			CustomPhysicalProperties = props(0.3, 1, s[2], s[3]),
		})
		table.insert(parts, b)
		drops[s[1]] = b
	end
	h.beats(30)
	local x0 = {}
	for name, b in blocks do
		x0[name] = b.Position.X
		b.AssemblyLinearVelocity = Vector3.new(20, 0, 0)
	end
	for _, b in drops do
		b.Anchored = false
		b:SetNetworkOwner(nil)
	end
	local result = {}
	local apex, landed = {}, {}
	for i = 1, 150 do
		h.beat()
		for name, b in drops do
			local y = b.Position.Y - origin.Y - 1.5
			if not landed[name] and y < 0.2 then
				landed[name] = i
			elseif landed[name] and i > landed[name] + 1 and y > (apex[name] or -1) then
				apex[name] = y
			end
		end
		if i == 20 then
			for name, b in blocks do
				result[name .. "_v20"] = h.round(b.AssemblyLinearVelocity.X, 2)
			end
		end
	end
	for name, b in blocks do
		result[name .. "_dist"] = h.round(b.Position.X - x0[name], 2)
	end
	for name in drops do
		result[name .. "_apex"] = h.round(apex[name] or 0, 2)
	end
	h.clear(parts)
	return result
end
