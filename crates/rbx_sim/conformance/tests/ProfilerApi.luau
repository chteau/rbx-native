-- The profiling and memory APIs a game script can reach on a server:
-- debug.profilebegin/profileend and the memory categories (argument
-- errors, unbalanced calls), Stats' memory figures and counters, and what
-- a game script gets from the plugin-only ScriptProfilerService.
return function(h)
	local Stats = game:GetService("Stats")
	local r = {}
	local function err(f, ...)
		local ok, message = pcall(f, ...)
		return ok and "ok" or (tostring(message):gsub("ServerScriptService%.Conformance%.", ""))
	end
	r.begin_ok = err(function()
		debug.profilebegin("label")
		debug.profileend()
	end)
	r.begin_none = err(debug.profilebegin)
	r.begin_number = err(debug.profilebegin, 5)
	r.begin_table = err(debug.profilebegin, {})
	r.begin_long = err(function()
		debug.profilebegin(string.rep("x", 1000))
		debug.profileend()
	end)
	r.end_unbalanced = err(debug.profileend)
	r.category_ok = err(function()
		debug.setmemorycategory("Conformance")
		debug.resetmemorycategory()
	end)
	r.category_none = err(debug.setmemorycategory)
	r.category_number = err(debug.setmemorycategory, 5)
	local returned = table.pack(debug.setmemorycategory("Conformance"))
	r.category_returns = returned.n .. ":" .. (returned.n > 0 and typeof(returned[1]) or "")
	debug.resetmemorycategory()
	r.category_first = tostring(debug.setmemorycategory("First"))
	r.category_second = tostring(debug.setmemorycategory("Second"))
	debug.resetmemorycategory()
	r.category_after_reset = tostring(debug.setmemorycategory("Third"))
	debug.resetmemorycategory()

	r.total_type = typeof(Stats:GetTotalMemoryUsageMb())
	r.total_positive = Stats:GetTotalMemoryUsageMb() > 0
	r.heap_positive = Stats:GetMemoryUsageMbForTag(Enum.DeveloperMemoryTag.LuaHeap) > 0
	r.script_positive = Stats:GetMemoryUsageMbForTag(Enum.DeveloperMemoryTag.Script) > 0
	r.tag_string = err(Stats.GetMemoryUsageMbForTag, Stats, "LuaHeap")
	r.tag_nil = err(Stats.GetMemoryUsageMbForTag, Stats)
	r.instance_count_positive = Stats.InstanceCount > 0
	r.primitives_type = typeof(Stats.PrimitivesCount)
	r.heartbeat_time_type = typeof(Stats.HeartbeatTime)
	r.heartbeat_time_sane = Stats.HeartbeatTime >= 0 and Stats.HeartbeatTime < 1
	r.physics_step_type = typeof(Stats.PhysicsStepTime)

	local profiler = game:GetService("ScriptProfilerService")
	r.sps_server_start = err(profiler.ServerStart, profiler)
	r.sps_server_stop = err(profiler.ServerStop, profiler)
	r.sps_request = err(profiler.ServerRequestData, profiler)
	r.sps_deserialize = err(profiler.DeserializeJSON, profiler, "{}")
	r.sps_event = err(function()
		return profiler.OnNewData
	end)
	h.beat()
	return r
end
