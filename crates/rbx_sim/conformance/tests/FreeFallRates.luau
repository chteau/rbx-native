-- Blocks of several sizes and starting velocities falling for 90 frames,
-- traced per frame (_<name>_vy, _<name>_y) to work out when Roblox's
-- adaptive timestepping moves an assembly between 60, 120 and 240 Hz.
-- kick_<name> is the first frame whose Y velocity change isn't one frame
-- of gravity (-1 when there's none).
return function(h, origin)
	local cases = {
		{ "s2", 2, Vector3.zero },
		{ "s1", 1, Vector3.zero },
		{ "s4", 4, Vector3.zero },
		{ "s8", 8, Vector3.zero },
		{ "down40", 2, Vector3.new(0, -40, 0) },
		{ "side60", 2, Vector3.new(60, 0, 0) },
		{ "up80", 2, Vector3.new(0, 80, 0) },
	}
	local parts = {}
	for i, case in cases do
		local part = h.part({
			Name = case[1],
			Size = Vector3.new(case[2], case[2], case[2]),
			Position = origin + Vector3.new(i * 20, 200, 0),
		})
		part.AssemblyLinearVelocity = case[3]
		parts[i] = part
	end
	local traces = {}
	for i = 1, #cases do
		traces[i] = { vy = {}, y = {} }
	end
	local dts = {}
	for _ = 1, 90 do
		table.insert(dts, h.round(h.beat(), 5))
		for i, part in parts do
			table.insert(traces[i].vy, h.round(part.AssemblyLinearVelocity.Y, 3))
			table.insert(traces[i].y, h.round(part.Position.Y - origin.Y - 200, 3))
		end
	end
	local g = workspace.Gravity / 60
	local r = { _dt = dts }
	for i, case in cases do
		local vy = traces[i].vy
		local kick = -1
		for f = 3, #vy do
			if math.abs(vy[f] - vy[f - 1] + g) > 0.01 then
				kick = f
				break
			end
		end
		r["kick_" .. case[1]] = kick
		r["_" .. case[1] .. "_vy"] = vy
		r["_" .. case[1] .. "_y"] = traces[i].y
	end
	h.clear(parts)
	return r
end
