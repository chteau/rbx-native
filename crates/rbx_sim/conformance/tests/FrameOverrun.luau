-- How a frame that overran shows in the frames after it: the step
-- Heartbeat reports, how far time() moves and how far physics moves (does
-- the world catch up the lost time, or skip it?), for one 2 s stall and for
-- 0.25 s of busy work inside each of four Heartbeats. The body moves at a
-- held 10 studs/s, so its travel is the simulated time x 10.
--
-- No script timeout is probed: a live server has none, a loop that never
-- yields hangs it for good (seen twice while recording: a 12 s busy loop
-- never came back), so nothing here runs longer than 2 s.
return function(h, origin)
	local RunService = game:GetService("RunService")
	local r = {}
	local function busy(seconds)
		local t0 = os.clock()
		while os.clock() - t0 < seconds do
		end
	end
	local part = h.part({ Size = Vector3.new(2, 2, 2), Position = origin })
	local velocity = h.new("LinearVelocity", {
		Attachment0 = h.attach(part),
		MaxForce = math.huge,
		VectorVelocity = Vector3.new(10, 0, 0),
	}, part)
	h.beats(10)

	local x0 = part.Position.X
	local time0 = time()
	local t0 = os.clock()
	busy(2)
	r.stall_elapsed = h.round(os.clock() - t0, 1)
	local dt = RunService.Heartbeat:Wait()
	r.stall_heartbeat_dt = h.round(dt, 2)
	r.stall_time_advance = h.round(time() - time0, 2)
	r.stall_travel = h.round(part.Position.X - x0, 1)
	local after = {}
	for _ = 1, 3 do
		table.insert(after, h.round(RunService.Heartbeat:Wait(), 3))
	end
	r.after_stall_dts = table.concat(after, " ")

	-- 0.25 s of busy work in each of 4 Heartbeats.
	h.beat()
	local dts, advances, travels = {}, {}, {}
	local count = 0
	local lastTime, lastX = time(), part.Position.X
	local conn = RunService.Heartbeat:Connect(function(step)
		table.insert(dts, h.round(step, 2))
		table.insert(advances, h.round(time() - lastTime, 2))
		table.insert(travels, h.round(part.Position.X - lastX, 1))
		lastTime, lastX = time(), part.Position.X
		count += 1
		if count <= 4 then
			busy(0.25)
		end
	end)
	h.beats(7)
	conn:Disconnect()
	r.overrun_dts = table.concat(dts, " ")
	r.overrun_time = table.concat(advances, " ")
	r.overrun_travel = table.concat(travels, " ")
	velocity:Destroy()
	part:Destroy()
	return r
end
