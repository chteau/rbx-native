-- Parallel Luau: task.desynchronize/synchronize, ConnectParallel and
-- BindToSimulationParallel from a script outside an Actor (this one) and
-- inside one (the suite's ConfActor.ParallelProbe, which starts with the
-- place and leaves what it saw in the Actor's attributes), and SharedTable.
return function(h)
	local result = {}
	local RunService = game:GetService("RunService")
	result.desync_outside = h.try(task.desynchronize)
	result.sync_outside = h.try(task.synchronize)
	result.connect_parallel_outside = h.try(function()
		local connection = RunService.Heartbeat:ConnectParallel(function() end)
		connection:Disconnect()
		return "ok"
	end)
	result.bind_outside = h.try(function()
		return RunService:BindToSimulationParallel(function() end)
	end)

	local actor = game:GetService("ServerScriptService"):WaitForChild("ConfActor")
	local waited = 0
	while not actor:GetAttribute("done") and waited < 600 do
		h.beat()
		waited += 1
	end
	local keys = {}
	for key in actor:GetAttributes() do
		table.insert(keys, key)
	end
	table.sort(keys)
	for _, key in keys do
		result["actor." .. key] = h.show(actor:GetAttribute(key))
	end

	-- SharedTable
	local st = SharedTable.new({ a = 1, b = "two" })
	result.st_type = typeof(st)
	result.st_a = h.show(st.a)
	result.st_missing = h.show(st.zzz)
	result.st_size = SharedTable.size(st)
	st.c = { 1, 2, 3 }
	result.st_nested_type = typeof(st.c)
	result.st_nested_size = SharedTable.size(st.c)
	result.st_increment = h.try(SharedTable.increment, st, "a", 5)
	result.st_after_increment = h.show(st.a)
	result.st_increment_string = h.try(SharedTable.increment, st, "b", 1)
	SharedTable.update(st, "a", function(v)
		return v * 10
	end)
	result.st_after_update = h.show(st.a)
	result.st_function_value = h.try(function()
		st.f = function() end
	end)
	result.st_instance_value = h.try(function()
		st.i = workspace
	end)
	result.st_bool_key = h.try(function()
		st[true] = 1
	end)
	result.st_float_key = h.try(function()
		st[1.5] = 1
	end)
	local frozen = SharedTable.cloneAndFreeze(st)
	result.st_frozen = tostring(SharedTable.isFrozen(frozen)) .. " " .. tostring(SharedTable.isFrozen(st))
	result.st_frozen_write = h.try(function()
		frozen.a = 2
	end)
	local copy = SharedTable.clone(st, false)
	copy.a = 99
	result.st_clone_independent = h.show(st.a) .. " " .. h.show(copy.a)
	local sum, count = 0, 0
	for key, value in SharedTable.new({ x = 1, y = 2, z = 3 }) do
		count += 1
		sum += value
	end
	result.st_iterate = count .. " " .. sum
	SharedTable.clear(copy)
	result.st_cleared = SharedTable.size(copy)
	result.st_tostring = (tostring(st):gsub("0x%x+", "ADDR"))
	result.st_registry = h.try(function()
		local registry = game:GetService("SharedTableRegistry")
		registry:SetSharedTable("conf", st)
		return registry:GetSharedTable("conf").a
	end)
	result.st_new_bad = h.try(SharedTable.new, 5)
	result.st_len = h.try(function()
		return #SharedTable.new({ 1, 2, 3 })
	end)
	return result
end
