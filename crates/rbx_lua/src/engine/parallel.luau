-- Parallel Luau as one VM runs it: serially, with the parallel phase a
-- per-thread flag the engine checks on every write (see `parallel.rs`),
-- and SharedTable as a proxy over a plain table. Messages are a live
-- server's (the ParallelLuau golden).
local setParallel, newProxy = ...

-- The script of the nearest caller that has one, skipping C frames and
-- the engine's own chunks.
local function callingScript()
	for level = 3, 12 do
		local ok, env = pcall(getfenv, level)
		if not ok then
			return nil
		end
		local script = rawget(env, "script")
		if typeof(script) == "Instance" then
			return script
		end
	end
	return nil
end

local function underActor()
	local script = callingScript()
	return script ~= nil and script:FindFirstAncestorWhichIsA("Actor") ~= nil
end

-- `f` running in the parallel phase, back to serial once it returns.
local function inParallel(f)
	return function(...)
		setParallel(true)
		local result = table.pack(pcall(f, ...))
		setParallel(false)
		if not result[1] then
			error(result[2], 0)
		end
		return table.unpack(result, 2, result.n)
	end
end

local parallel = { inParallel = inParallel, underActor = underActor }

function parallel.desynchronize()
	if not underActor() then
		error("task.desynchronize() may only be called from a script that is a descendant of an Actor", 2)
	end
	setParallel(true)
end

function parallel.synchronize()
	if not underActor() then
		error("task.synchronize() may only be called from a script that is a descendant of an Actor", 2)
	end
	setParallel(false)
end

-- SharedTable
local states = setmetatable({}, { __mode = "k" })
local mt = {}
local counter = 0x2a7c0000
local SharedTable = {}

local function state(st, name)
	local s = states[st]
	if s == nil then
		error(string.format("invalid argument #1 to '%s' (SharedTable expected, got %s)", name, typeof(st)), 3)
	end
	return s
end

local function checkKey(key)
	if type(key) == "number" then
		if key < 0 or key >= 2 ^ 32 or key % 1 ~= 0 then
			error("Numeric SharedTable keys must be nonnegative integers less than 2^32", 3)
		end
	elseif type(key) ~= "string" then
		error("SharedTable keys can only be strings or numbers.", 3)
	end
end

local make

local function convert(value, frozen)
	local kind = typeof(value)
	if kind == "function" then
		error("Functions can not be stored in a SharedTable", 3)
	elseif kind == "Instance" then
		error("Instances can not be stored in a SharedTable", 3)
	elseif kind == "table" then
		return make(value, frozen)
	elseif kind == "thread" or (kind == "userdata" and states[value] == nil) then
		error(string.format("Values of type %s can not be stored in a SharedTable", kind), 3)
	end
	return value
end

function make(source, frozen)
	local st = newProxy()
	local data = {}
	counter += 0x40
	states[st] = { data = data, frozen = frozen or false, id = counter }
	for key, value in source or {} do
		checkKey(key)
		data[key] = convert(value, frozen)
	end
	return st
end

mt.__index = function(st, key)
	return states[st].data[key]
end
mt.__newindex = function(st, key, value)
	local s = states[st]
	if s.frozen then
		error("SharedTable is frozen", 2)
	end
	checkKey(key)
	s.data[key] = convert(value, false)
end
mt.__iter = function(st)
	return next, states[st].data
end
mt.__tostring = function(st)
	return string.format("SharedTable: 0x%08x", states[st].id)
end

function SharedTable.new(source)
	if source ~= nil and type(source) ~= "table" then
		error("invalid argument #1 to 'new' (table expected, got " .. typeof(source) .. ")", 2)
	end
	return make(source, false)
end

function SharedTable.size(st)
	local n = 0
	for _ in state(st, "size").data do
		n += 1
	end
	return n
end

function SharedTable.clear(st)
	local s = state(st, "clear")
	if s.frozen then
		error("SharedTable is frozen", 2)
	end
	table.clear(s.data)
end

local function copy(st, deep, frozen)
	local out = {}
	for key, value in states[st].data do
		out[key] = if deep and states[value] then copy(value, true, frozen) else value
	end
	local result = make(nil, frozen)
	for key, value in out do
		states[result].data[key] = value
	end
	return result
end

function SharedTable.clone(st, deep)
	state(st, "clone")
	return copy(st, deep, false)
end

function SharedTable.cloneAndFreeze(st, deep)
	state(st, "cloneAndFreeze")
	return copy(st, deep, true)
end

function SharedTable.isFrozen(st)
	return state(st, "isFrozen").frozen
end

function SharedTable.update(st, key, f)
	local s = state(st, "update")
	if s.frozen then
		error("SharedTable is frozen", 2)
	end
	checkKey(key)
	s.data[key] = convert(f(s.data[key]), false)
end

function SharedTable.increment(st, key, delta)
	local s = state(st, "increment")
	if s.frozen then
		error("SharedTable is frozen", 2)
	end
	local old = s.data[key]
	if old ~= nil and type(old) ~= "number" then
		error(string.format('Unable to increment the value for key "%s" because it is not a number value.', tostring(key)), 2)
	end
	s.data[key] = (old or 0) + delta
	return old or 0
end

parallel.SharedTable = table.freeze(SharedTable)
-- The proxies' metatable (`parallel.rs` builds it from these).
parallel.proxyMeta = mt

-- SharedTableRegistry, one per VM.
local registry = {}
function parallel.getShared(_, name)
	local st = registry[name]
	if st == nil then
		st = make(nil, false)
		registry[name] = st
	end
	return st
end
function parallel.setShared(_, name, st)
	if st ~= nil then
		state(st, "SetSharedTable")
	end
	registry[name] = st
end

return parallel
