-- A falling block's velocity sampled at Stepped, PostSimulation and
-- Heartbeat of each of its first 32 frames, and a second block made inside
-- Stepped (before that frame's physics). Fields starting with _ are traces
-- for working out the stepping, not compared.
return function(h, origin)
	local RunService = game:GetService("RunService")
	local part = h.part({
		Name = "Tracer",
		Size = Vector3.new(2, 2, 2),
		Position = origin + Vector3.new(0, 200, 0),
	})
	local y0 = part.Position.Y
	local frame = 0
	local st, post, hb, hby, dts = {}, {}, {}, {}, {}
	local late, lateY0, lateVy = nil, 0, {}
	local c1 = RunService.Stepped:Connect(function()
		st[frame + 1] = h.round(part.AssemblyLinearVelocity.Y, 3)
		if frame == 2 and not late then
			late = h.part({
				Name = "Late",
				Size = Vector3.new(2, 2, 2),
				Position = origin + Vector3.new(10, 200, 0),
			})
			lateY0 = late.Position.Y
		end
	end)
	local c2 = RunService.PostSimulation:Connect(function()
		post[frame + 1] = h.round(part.AssemblyLinearVelocity.Y, 3)
	end)
	for i = 1, 32 do
		local dt = h.beat()
		frame = i
		dts[i] = h.round(dt, 5)
		hb[i] = h.round(part.AssemblyLinearVelocity.Y, 3)
		hby[i] = h.round(part.Position.Y - y0, 3)
		if late then
			table.insert(lateVy, h.round(late.AssemblyLinearVelocity.Y, 3))
		end
	end
	c1:Disconnect()
	c2:Disconnect()
	local lateDy = h.round(late.Position.Y - lateY0, 3)
	part:Destroy()
	late:Destroy()
	return {
		hb_vy_1 = hb[1],
		hb_vy_2 = hb[2],
		hb_vy_4 = hb[4],
		hb_vy_16 = hb[16],
		hb_vy_30 = hb[30],
		hb_dy_1 = hby[1],
		hb_dy_30 = hby[30],
		st_vy_30 = st[30],
		post_vy_30 = post[30],
		late_vy_1 = lateVy[1],
		late_vy_30 = lateVy[30],
		late_dy_30 = lateDy,
		_st = st,
		_post = post,
		_hb = hb,
		_hby = hby,
		_dt = dts,
		_late = lateVy,
	}
end
