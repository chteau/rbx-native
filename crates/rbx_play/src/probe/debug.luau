local RunService = game:GetService("RunService")

local D = { s = false }

local side = if RunService:IsServer() then "server" else "client"
local isServer = side == "server"
local sender = nil
local pending = {}
local tables = {}
local steps = {}
local pauses = {}
local nextPause = 0
-- This module's own name in `debug.info`: its frames (`hit`, `stop`) are
-- left out of a pause's call stack.
local OWN_SOURCE = debug.info(1, "s")

local function send(message)
	if sender then
		sender(message)
	else
		table.insert(pending, message)
	end
end

local function updateStepping()
	D.s = next(steps) ~= nil
end

local function depthOf()
	local n = 0
	while debug.info(n + 1, "s") ~= nil do
		n += 1
	end
	return n
end

local function formatScalar(v)
	local t = typeof(v)
	if t == "string" then
		return (string.format("%q", v):sub(1, 200))
	elseif t == "nil" or t == "boolean" or t == "number" then
		return tostring(v)
	elseif t == "Instance" then
		return v.ClassName .. " " .. v:GetFullName()
	elseif t == "function" then
		return "function"
	end
	return t .. "(" .. tostring(v) .. ")"
end

local function format(v)
	if type(v) ~= "table" then
		return formatScalar(v)
	end
	local parts = {}
	local count = 0
	for k, item in pairs(v) do
		if count >= 8 then
			table.insert(parts, "…")
			break
		end
		count += 1
		local key = if type(k) == "string" then k else "[" .. formatScalar(k) .. "]"
		local text = if type(item) == "table" then "{…}" else formatScalar(item)
		table.insert(parts, key .. " = " .. text)
	end
	return "{" .. table.concat(parts, ", ") .. "}"
end

local function splitNames(names)
	local out = {}
	for name in string.gmatch(names, "%S+") do
		table.insert(out, name)
	end
	return out
end

local function localsOf(names, values)
	local map = {}
	for i, name in splitNames(names) do
		map[name] = values[i]
	end
	return map
end

function D.lines(id, serverLines, clientLines)
	local b = tables[id]
	if not b then
		b = {}
		tables[id] = b
	end
	for _, line in if isServer then serverLines else clientLines do
		b[line] = true
	end
	return b
end

local function collectFrames()
	local frames = {}
	local level = 2
	while true do
		local source, line, name = debug.info(level, "sln")
		if source == nil then
			break
		end
		if source ~= "[C]" and source ~= OWN_SOURCE and line >= 1 then
			table.insert(frames, { source = source, line = line, ["function"] = name or "" })
		end
		level += 1
	end
	return frames
end

local function stop(thread, id, line, names, values, depth)
	nextPause += 1
	local pause = nextPause
	local list = {}
	for i, name in splitNames(names) do
		table.insert(list, { name = name, value = format(values[i]) })
	end
	local entry = { names = names, values = values, thread = thread }
	pauses[pause] = entry
	send({
		type = "paused",
		side = side,
		pause = pause,
		script = id,
		line = line,
		frames = collectFrames(),
		locals = list,
	})
	repeat
		task.wait()
	until entry.action
	pauses[pause] = nil
	if entry.action == "continue" then
		steps[thread] = nil
	else
		steps[thread] = { action = entry.action, depth = depth }
	end
	updateStepping()
end

function D.hit(id, line, names, values, cond, log, cont)
	local thread = coroutine.running()
	local depth = depthOf()
	local step = steps[thread]
	local fire = false
	if step then
		if step.action == "step_into" then
			fire = true
		elseif step.action == "step_over" then
			fire = depth <= step.depth
		elseif step.action == "step_out" then
			fire = depth < step.depth
		end
	end
	if fire then
		steps[thread] = nil
		updateStepping()
	else
		local b = tables[id]
		if not (b and rawget(b, line) == true) then
			return
		end
		if cond then
			local ok, result = pcall(cond)
			if not ok then
				warn("[rbxnative] breakpoint condition: " .. tostring(result))
			elseif not result then
				return
			end
		end
		if log then
			pcall(log)
		end
		if cont then
			return
		end
	end
	if not coroutine.isyieldable() then
		warn("[rbxnative] line " .. tostring(line) .. " cannot pause: the thread cannot yield")
		return
	end
	stop(thread, id, line, names, values, depth)
end

function D.attach(newSide, fn)
	side = newSide
	isServer = side == "server"
	sender = fn
	local queued = pending
	pending = {}
	for _, message in queued do
		fn(message)
	end
end

local function resolvePath(expression, map)
	local env = getfenv(0)
	local text = expression:match("^%s*(.-)%s*$")
	local name, rest = text:match("^([%a_][%w_]*)(.*)$")
	if not name then
		error("cannot evaluate: " .. expression)
	end
	local value = map[name]
	if value == nil then
		value = env[name]
	end
	while rest ~= "" do
		local key
		local field, after = rest:match("^%.([%a_][%w_]*)(.*)$")
		if field then
			key, rest = field, after
		else
			local num, after2 = rest:match("^%[(%-?%d+)%](.*)$")
			if num then
				key, rest = tonumber(num), after2
			else
				local str, after3 = rest:match('^%["([^"]*)"%](.*)$')
				if not str then
					error("cannot evaluate: " .. expression)
				end
				key, rest = str, after3
			end
		end
		if type(value) ~= "table" and typeof(value) ~= "Instance" then
			error("attempt to index " .. typeof(value))
		end
		value = value[key]
	end
	return value
end

local function evaluate(expression, map, canCompile)
	if canCompile then
		local fn, err = loadstring("return " .. expression)
		if not fn then
			return false, tostring(err)
		end
		setfenv(fn, setmetatable(map, { __index = getfenv(0) }))
		local ok, result = pcall(fn)
		return ok, if ok then format(result) else tostring(result)
	end
	local ok, result = pcall(resolvePath, expression, map)
	return ok, if ok then format(result) else tostring(result)
end

function D.command(cmd)
	local kind = cmd.type
	if kind == "debug_set" then
		for _, b in tables do
			table.clear(b)
		end
		for _, entry in cmd.lines or {} do
			local b = tables[entry.script]
			if not b then
				b = {}
				tables[entry.script] = b
			end
			b[entry.line] = true
		end
	elseif kind == "debug_resume" then
		local entry = pauses[cmd.pause]
		local action = cmd.action
		if
			entry
			and (action == "continue" or action == "step_into" or action == "step_over" or action == "step_out")
		then
			entry.action = action
		end
	elseif kind == "debug_watch" then
		local entry = pauses[cmd.pause]
		local map = if entry then localsOf(entry.names, entry.values) else {}
		local compiled, chunk = pcall(loadstring, "return 1")
		local canCompile = compiled and type(chunk) == "function"
		local out = {}
		for _, expression in cmd.expressions or {} do
			local ok, text = evaluate(expression, map, canCompile)
			table.insert(out, { expression = expression, ok = ok, text = text })
		end
		send({ type = "debug_values", side = side, pause = cmd.pause, values = out })
	end
end

return D
