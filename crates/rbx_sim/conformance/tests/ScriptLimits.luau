-- The Luau and engine limits a game script runs into on a server, and the
-- exact words of each error: stack overflow (depth and text), the
-- re-entrancy caps on task.defer and on events, string, buffer and table
-- size limits, GetService on bad names, require's errors (a cycle, a
-- module that errored, one returning nothing), the legacy wait/spawn/delay
-- timing and how many legacy waits resume in one frame, and task.wait's
-- granularity for waits under a frame.
return function(h)
	local RunService = game:GetService("RunService")
	local r = {}
	local logs = {}
	local function clean(message)
		return (tostring(message):gsub("ServerScriptService%.Conformance%.", ""))
	end
	local function err(f, ...)
		local ok, message = pcall(f, ...)
		return ok and "ok" or clean(message)
	end
	local errors = game:GetService("ScriptContext").Error:Connect(function(message)
		table.insert(logs, "error " .. clean(message))
	end)
	local okLog, messageOut = pcall(function()
		return game:GetService("LogService").MessageOut:Connect(function(message, kind)
			table.insert(logs, kind.Name .. " " .. clean(message))
		end)
	end)
	-- What was reported since `from`, joined.
	local function since(from)
		local out = {}
		for i = from + 1, #logs do
			table.insert(out, logs[i])
		end
		return table.concat(out, " | ")
	end

	-- Stack overflow: plain recursion, through pcall, through __index.
	local depth = 0
	local function recurse()
		depth += 1
		recurse()
	end
	r.stack_msg = err(recurse)
	r.stack_depth = depth
	depth = 0
	local function viaPcall()
		depth += 1
		local ok, message = pcall(viaPcall)
		if not ok then
			error(message, 0)
		end
	end
	r.pcall_stack_msg = err(viaPcall)
	r.pcall_stack_depth = depth
	depth = 0
	local meta = {}
	meta.__index = function(t, k)
		depth += 1
		return t[k]
	end
	r.index_stack_msg = err(function()
		return setmetatable({}, meta).x
	end)
	r.index_stack_depth = depth

	-- task.defer deferring itself, and an event firing itself.
	h.beat()
	local from = #logs
	local deferred = 0
	local function again()
		deferred += 1
		if deferred < 1000 then
			task.defer(again)
		end
	end
	task.defer(again)
	h.beat()
	r.defer_count_beat1 = deferred
	h.beats(3)
	r.defer_count = deferred
	r.defer_logs = since(from)

	from = #logs
	local hits = 0
	local plain = Instance.new("BindableEvent")
	plain.Event:Connect(function()
		hits += 1
	end)
	plain:Fire()
	r.plain_fire_sync = hits
	local fired = 0
	local event = Instance.new("BindableEvent")
	event.Event:Connect(function()
		fired += 1
		if fired < 1000 then
			event:Fire()
		end
	end)
	event:Fire()
	r.event_count_sync = fired
	h.beat()
	r.event_count_beat1 = fired
	h.beats(3)
	r.event_count = fired
	r.event_logs = since(from)

	from = #logs
	local changes = 0
	local value = Instance.new("IntValue")
	value.Changed:Connect(function()
		changes += 1
		if changes < 1000 then
			value.Value += 1
		end
	end)
	value.Value = 1
	h.beats(4)
	r.changed_count = changes
	r.changed_logs = since(from)

	-- Size limits that fail before allocating anything.
	r.rep_overflow = err(string.rep, "ab", 2 ^ 30)
	r.rep_negative = #string.rep("x", -1)
	r.buffer_big = err(buffer.create, 2 ^ 30 + 1)
	r.buffer_negative = err(buffer.create, -1)
	r.table_create_big = err(table.create, 2 ^ 27)
	r.table_create_negative = err(table.create, -1)
	r.char_range = err(string.char, 256)
	r.format_width = err(string.format, "%100d", 1)
	r.concat_type = err(table.concat, { 1, {}, 3 })
	r.unpack_many = err(table.unpack, {}, 1, 1e7)

	-- GetService with names that aren't services.
	r.service_bogus = err(game.GetService, game, "NotAService")
	r.service_empty = err(game.GetService, game, "")
	local okPart, part = pcall(game.GetService, game, "Part")
	r.service_part = okPart and typeof(part) .. ":" .. tostring(part and part.Parent) or clean(part)
	r.service_number = err(game.GetService, game, 5)
	r.find_service_bogus = err(game.FindService, game, "NotAService")

	-- require's errors.
	local tests = script.Parent
	r.require_cycle = err(require, tests:WaitForChild("LimitsCycleA"))
	r.require_error = err(require, tests:WaitForChild("LimitsBroken"))
	r.require_error_again = err(require, tests:WaitForChild("LimitsBroken"))
	r.require_empty = err(require, tests:WaitForChild("LimitsEmpty"))
	r.require_folder = err(require, workspace)
	r.require_nil = err(require, nil)
	r.require_bogus_id = err(require, -5)

	-- Legacy wait/spawn/delay: frames taken, what they hand back.
	local beats = 0
	local counter = RunService.Heartbeat:Connect(function()
		beats += 1
	end)
	h.beat()
	local b0 = beats
	local spawnArgs
	spawn(function(...)
		spawnArgs = table.pack(...)
		spawnArgs.beats = beats - b0
	end)
	local delayBeats = -1
	delay(0, function()
		delayBeats = beats - b0
	end)
	local waited, gameTime = wait(0)
	r.wait0_beats = beats - b0
	r.wait0_second = typeof(gameTime)
	r.wait0_elapsed_ok = waited > 0 and waited < 0.1
	h.beats(3)
	r.spawn_beats = spawnArgs and spawnArgs.beats or -1
	r.spawn_argc = spawnArgs and spawnArgs.n or -1
	r.spawn_arg1 = spawnArgs and typeof(spawnArgs[1]) or "none"
	r.delay0_beats = delayBeats
	-- 3000 legacy waits from the same frame: how many resume per frame.
	h.beat()
	local resumed = {}
	b0 = beats
	for _ = 1, 3000 do
		spawn(function()
			wait()
			local frame = beats - b0
			resumed[frame] = (resumed[frame] or 0) + 1
		end)
	end
	h.beats(10)
	local frames = {}
	for frame, count in resumed do
		table.insert(frames, frame .. ":" .. count)
	end
	table.sort(frames)
	r.legacy_resume_frames = table.concat(frames, " ")

	-- task.wait under a frame and task.delay(0).
	for _, case in { { "0001", 0.001 }, { "60th", 1 / 60 }, { "120th", 1 / 120 }, { "30th", 1 / 30 } } do
		h.beat()
		b0 = beats
		task.wait(case[2])
		r["task_wait_" .. case[1] .. "_beats"] = beats - b0
	end
	h.beat()
	b0 = beats
	local taskDelay = -1
	task.delay(0, function()
		taskDelay = beats - b0
	end)
	h.beats(3)
	r.task_delay0_beats = taskDelay
	r.task_wait_string = err(task.wait, "x")
	counter:Disconnect()
	errors:Disconnect()
	if okLog then
		messageOut:Disconnect()
	end
	r.log_service = okLog
	return r
end
