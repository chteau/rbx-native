-- Shared by the server and client halves of the client/server tests: a
-- canonical description of any value, and the payloads RemoteArgs sends
-- both ways.
local Shared = {}

local function sortKey(k)
	return typeof(k) .. ":" .. tostring(k)
end

local function describe(v, seen)
	local kind = typeof(v)
	if kind == "nil" then
		return "nil"
	elseif kind == "number" then
		if v ~= v then
			return "nan"
		elseif v == math.huge then
			return "inf"
		elseif v == -math.huge then
			return "-inf"
		elseif v == 0 and 1 / v < 0 then
			return "-0"
		end
		return string.format("%.14g", v)
	elseif kind == "string" then
		return string.format("%q", v)
	elseif kind == "boolean" then
		return tostring(v)
	elseif kind == "Instance" then
		return "Instance:" .. v.Name
	elseif kind == "EnumItem" then
		return tostring(v)
	elseif kind == "buffer" then
		return "buffer:" .. buffer.len(v)
	elseif kind == "table" then
		if seen[v] then
			return "<seen>"
		end
		seen[v] = true
		local keys = {}
		for k in pairs(v) do
			table.insert(keys, k)
		end
		table.sort(keys, function(a, b)
			return sortKey(a) < sortKey(b)
		end)
		local parts = {}
		for _, k in keys do
			table.insert(parts, "[" .. typeof(k) .. " " .. tostring(k) .. "]=" .. describe(v[k], seen))
		end
		seen[v] = nil
		return "{" .. table.concat(parts, ",") .. "}" .. (getmetatable(v) and "+mt" or "")
	end
	return kind
end

function Shared.describe(v)
	return describe(v, {})
end

-- Everything a handler got: how many values, then each described.
function Shared.describeArgs(...)
	local n = select("#", ...)
	local parts = { "n=" .. n }
	for i = 1, n do
		table.insert(parts, describe((select(i, ...)), {}))
	end
	return table.concat(parts, " ")
end

-- Name, then the values to send. `own` is an instance only the sender has,
-- `hidden` one that never replicates (the server's ServerStorage), nil on
-- the client.
function Shared.payloads(own, hidden)
	local shared = { 1 }
	local buf = buffer.create(3)
	buffer.writeu8(buf, 0, 7)
	return {
		{ "array", { { 1, 2, 3 } } },
		{ "mixed", { { 1, 2, a = "x" } } },
		{ "hole", { { 1, nil, 3 } } },
		{ "gap", { { [1] = 1, [2] = 2, [4] = 4 } } },
		{ "sparse", { { [10] = "a", [20] = "b" } } },
		{ "zero_neg", { { [0] = "z", [-1] = "m" } } },
		{ "float_key", { { [1.5] = "f" } } },
		{ "bool_key", { { [true] = "t" } } },
		{ "inst_key", { { [workspace] = "w" } } },
		{ "vec_key", { { [Vector3.new(1, 2, 3)] = "v" } } },
		{ "str_num_keys", { { ["1"] = "a", ["2"] = "b" } } },
		{ "nan_value", { { x = 0 / 0 } } },
		{ "nan", { 0 / 0 } },
		{ "inf", { math.huge } },
		{ "neg_zero", { -0 } },
		{ "big", { 2 ^ 53 + 1 } },
		{ "meta", { setmetatable({ a = 1 }, { __index = function() return 5 end }) } },
		{ "func_field", { { f = print, g = 1 } } },
		{ "func", { print } },
		{ "nested", { { a = { b = { c = 1 } } } } },
		{ "same_table", { { shared, shared } } },
		{ "empty", { {} } },
		{ "nul", { "a\0b" } },
		{ "trailing_nils", { 1, nil, nil, n = 3 } },
		{ "vector3", { Vector3.new(1, 2, 3) } },
		{ "cframe", { CFrame.new(1, 2, 3) } },
		{ "color3", { Color3.new(1, 0.5, 0) } },
		{ "udim2", { UDim2.new(0.5, 1, 0.25, 2) } },
		{ "enum", { Enum.Material.Wood } },
		{ "brick", { BrickColor.new("Bright red") } },
		{ "numseq", { NumberSequence.new(0, 1) } },
		{ "buffer", { buf } },
		{ "own_inst", { own, n = 1 } },
		{ "hidden_inst", { hidden, n = 1 } },
		{ "shared_inst", { workspace:FindFirstChild("Baseplate") } },
		{ "inst_in_table", { { own, workspace:FindFirstChild("Baseplate") } } },
	}
end

-- Sends every payload through `fire(name, ...)`: how many went, and the
-- error each one that didn't raised, by name.
function Shared.sendAll(fire, own, hidden)
	local sent, errors = 0, {}
	for _, entry in Shared.payloads(own, hidden) do
		local values = entry[2]
		local ok, message = pcall(fire, entry[1], table.unpack(values, 1, values.n or #values))
		if ok then
			sent += 1
		else
			errors[entry[1]] = (tostring(message):gsub("^.-:%d+: ", ""))
		end
	end
	return sent, errors
end

-- The first few words of an error, as Roblox words them.
function Shared.err(f)
	local ok, message = pcall(f)
	if ok then
		return "ok"
	end
	message = tostring(message)
	return (message:gsub("^.-:%d+: ", ""))
end

-- Shared.err for a call that may never return: "yields" when it has not
-- after 30 calls of `beat`.
function Shared.errOrYield(f, beat)
	local result
	task.spawn(function()
		result = Shared.err(f)
	end)
	for _ = 1, 30 do
		if result then
			break
		end
		beat()
	end
	return result or "yields"
end

-- A position to the stud.
function Shared.vec(v)
	local function r(x)
		return math.floor(x + 0.5)
	end
	return string.format("%d,%d,%d", r(v.X), r(v.Y), r(v.Z))
end

return Shared
