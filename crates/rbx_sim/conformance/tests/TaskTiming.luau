-- How many heartbeats task.wait, wait and task.delay take, and what the
-- waits hand back.
return function(h)
	local beats = 0
	local conn = game:GetService("RunService").Heartbeat:Connect(function()
		beats += 1
	end)
	h.beat()
	local r = {}
	local b0 = beats
	local e = task.wait()
	r.wait0_beats, r.wait0 = beats - b0, h.round(e, 3)
	b0 = beats
	e = task.wait(0.1)
	r.wait01_beats, r.wait01 = beats - b0, h.round(e, 3)
	b0 = beats
	e = task.wait(0.5)
	r.wait05_beats, r.wait05 = beats - b0, h.round(e, 3)
	b0 = beats
	local legacy = wait()
	r.legacy_wait_beats, r.legacy_wait = beats - b0, h.round(legacy, 3)
	b0 = beats
	local fired = -1
	task.delay(0.25, function()
		fired = beats - b0
	end)
	task.wait(0.5)
	r.delay025_beats = fired
	conn:Disconnect()
	return r
end
