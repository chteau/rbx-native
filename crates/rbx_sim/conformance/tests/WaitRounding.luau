-- Heartbeats taken by task.wait, legacy wait and task.delay for a range
-- of times, each started where the last resumed, plus the Heartbeat step
-- times summed over each wait (_dt_*) to see how long a frame really is.
return function(h)
	local RunService = game:GetService("RunService")
	local beats, sum = 0, 0
	local conn = RunService.Heartbeat:Connect(function(dt)
		beats += 1
		sum += dt
	end)
	h.beat()
	local r = {}
	local function key(t)
		return (string.gsub(tostring(t), "%.", "_"))
	end
	local function measure(name, t, f)
		local b0, s0 = beats, sum
		local e = f(t)
		r[name .. key(t) .. "_beats"] = beats - b0
		if e then
			r[name .. key(t)] = h.round(e, 4)
		end
		r["_dt_" .. name .. key(t)] = h.round(sum - s0, 5)
	end
	for _, t in { 0.05, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.75, 1, 1.5, 2 } do
		measure("task", t, task.wait)
	end
	for _, t in { 0.1, 0.5, 1 } do
		measure("legacy", t, wait)
	end
	for _, t in { 0.5, 1 } do
		measure("delay", t, function(t)
			local done = false
			task.delay(t, function()
				done = true
			end)
			repeat
				RunService.Heartbeat:Wait()
			until done
			return nil
		end)
	end
	-- The same wait started from inside Stepped instead of after a resume.
	local b0, started, stepped = 0, false, nil
	stepped = RunService.Stepped:Connect(function()
		if started then
			return
		end
		started = true
		stepped:Disconnect()
		task.spawn(function()
			b0 = beats
			task.wait(0.5)
			r.stepped0_5_beats = beats - b0
		end)
	end)
	repeat
		RunService.Heartbeat:Wait()
	until r.stepped0_5_beats
	conn:Disconnect()
	return r
end
