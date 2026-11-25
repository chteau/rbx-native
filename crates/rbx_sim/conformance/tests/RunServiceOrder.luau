-- The order RunService's events, waiting threads and deferred calls run in
-- over the two frames after a heartbeat.
return function(h)
	local RunService = game:GetService("RunService")
	local log, recording = {}, false
	local conns = {}
	for _, name in { "PreAnimation", "PreSimulation", "Stepped", "PostSimulation", "Heartbeat" } do
		table.insert(
			conns,
			RunService[name]:Connect(function()
				if recording then
					table.insert(log, name)
				end
			end)
		)
	end
	h.beat()
	recording = true
	task.spawn(function()
		task.wait()
		table.insert(log, "task.wait")
	end)
	task.defer(function()
		table.insert(log, "defer")
	end)
	task.delay(0, function()
		table.insert(log, "delay0")
	end)
	table.insert(log, "|")
	h.beat()
	table.insert(log, "beat1")
	h.beat()
	table.insert(log, "beat2")
	recording = false
	for _, c in conns do
		c:Disconnect()
	end
	return { order = table.concat(log, " ") }
end
