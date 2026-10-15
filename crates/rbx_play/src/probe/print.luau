local Print = {}

local FIFO_CAP = 256
local MAX_ENTRIES = 200
local MAX_DEPTH = 8
local NODE_BUDGET = 2000

local fifo = {}

local function walkable(v)
	if type(v) ~= "table" then
		return false
	end
	local mt = getmetatable(v)
	if mt == nil then
		return true
	end
	if type(mt) ~= "table" then
		return false
	end
	return rawget(mt, "__tostring") == nil
end

local function addressOf(t)
	local s = tostring(t)
	return string.match(s, "^table: (.+)$") or s
end

local function rank(k)
	local kind = type(k)
	if kind == "number" then
		return 1
	elseif kind == "string" then
		return 2
	end
	return 3
end

local function keyBefore(a, b)
	if a.rank ~= b.rank then
		return a.rank < b.rank
	end
	if a.rank < 3 and a.key ~= b.key then
		return a.key < b.key
	end
	return a.index < b.index
end

local function keyText(k)
	if type(k) == "string" then
		return string.format("%q", k)
	end
	return tostring(k)
end

local function dump(t, depth, open, state)
	local address = addressOf(t)
	open[t] = true
	local items = {}
	local more = 0
	local index = 0
	for k, v in pairs(t) do
		index += 1
		if index <= MAX_ENTRIES then
			items[index] = { key = k, value = v, rank = rank(k), index = index }
		else
			more += 1
		end
	end
	table.sort(items, keyBefore)
	local entries = {}
	for _, item in ipairs(items) do
		state.budget -= 1
		local v = item.value
		local value
		if type(v) == "string" then
			value = string.format("%q", v)
		elseif walkable(v) then
			if open[v] or depth + 1 >= MAX_DEPTH or state.budget <= 0 then
				value = { reference = addressOf(v) }
			else
				value = dump(v, depth + 1, open, state)
			end
		else
			value = tostring(v)
		end
		entries[#entries + 1] = { key = keyText(item.key), value = value }
	end
	open[t] = nil
	return { address = address, entries = entries, more = more }
end

local function buildParts(args, n)
	local any = false
	for i = 1, n do
		if walkable(args[i]) then
			any = true
			break
		end
	end
	if not any then
		return nil
	end
	local state = { budget = NODE_BUDGET }
	local parts = {}
	for i = 1, n do
		local v = args[i]
		if walkable(v) then
			parts[i] = dump(v, 0, {}, state)
		else
			parts[i] = tostring(v)
		end
	end
	return parts
end

local function cleanSource(s)
	return string.match(s, '^%[string "(.*)"%]$') or s
end

function Print.wrap(original)
	return function(...)
		local source, line = debug.info(2, "sl")
		pcall(function(...)
			local n = select("#", ...)
			local args = table.pack(...)
			local texts = {}
			for i = 1, n do
				texts[i] = tostring(args[i])
			end
			local record = { text = table.concat(texts, " ") }
			local ok, parts = pcall(buildParts, args, n)
			if ok then
				record.parts = parts
			end
			if source ~= nil and source ~= "" and line ~= nil and line >= 1 then
				record.origin = { script = cleanSource(source), line = line }
			end
			fifo[#fifo + 1] = record
			if #fifo > FIFO_CAP then
				table.remove(fifo, 1)
			end
		end, ...)
		return original(...)
	end
end

function Print.take(message)
	for i, record in ipairs(fifo) do
		if record.text == message then
			for _ = 1, i do
				table.remove(fifo, 1)
			end
			return record
		end
	end
	return nil
end

return Print
