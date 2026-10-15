--!strict
--! Minimal deterministic JSON codec: sorted object keys and canonical number
--! formatting so both ends of the relay produce byte-identical text.

local Json = {}

Json.null = setmetatable({}, {
	__tostring = function()
		return "null"
	end,
})

local arrays: {[any]: boolean} = setmetatable({}, { __mode = "k" }) :: any

function Json.array(t: {any}?): {any}
	local result: {any} = t or {}
	arrays[result] = true
	return result
end

function Json.isArray(t: any): boolean
	if arrays[t] then
		return true
	end
	local n = #t
	if n == 0 then
		return false
	end
	local count = 0
	for _ in pairs(t) do
		count += 1
	end
	return count == n
end

local ESCAPES: {[string]: string} = {
	['"'] = '\\"',
	["\\"] = "\\\\",
	["\b"] = "\\b",
	["\f"] = "\\f",
	["\n"] = "\\n",
	["\r"] = "\\r",
	["\t"] = "\\t",
}

local function encodeString(s: string): string
	local escaped = s:gsub('[%c"\\]', function(c: string): string
		return ESCAPES[c] or string.format("\\u%04x", string.byte(c))
	end)
	return '"' .. escaped .. '"'
end

local function encodeNumber(n: number): string
	if n ~= n or n == math.huge or n == -math.huge then
		error("cannot encode non-finite number")
	end
	if n == 0 then
		return "0"
	end
	if n % 1 == 0 and math.abs(n) < 2 ^ 53 then
		return string.format("%.0f", n)
	end
	local short = string.format("%.14g", n)
	if tonumber(short) == n then
		return short
	end
	return string.format("%.17g", n)
end

local function encodeValue(value: any, out: {string})
	local kind = type(value)
	if value == Json.null or value == nil then
		table.insert(out, "null")
	elseif kind == "boolean" then
		table.insert(out, if value then "true" else "false")
	elseif kind == "number" then
		table.insert(out, encodeNumber(value))
	elseif kind == "string" then
		table.insert(out, encodeString(value))
	elseif kind == "table" then
		if Json.isArray(value) then
			table.insert(out, "[")
			for i = 1, #value do
				if i > 1 then
					table.insert(out, ",")
				end
				encodeValue(value[i], out)
			end
			table.insert(out, "]")
		else
			local keys: {string} = {}
			for k in pairs(value) do
				if type(k) ~= "string" then
					error("object keys must be strings")
				end
				table.insert(keys, k)
			end
			table.sort(keys)
			table.insert(out, "{")
			for i, k in ipairs(keys) do
				if i > 1 then
					table.insert(out, ",")
				end
				table.insert(out, encodeString(k))
				table.insert(out, ":")
				encodeValue(value[k], out)
			end
			table.insert(out, "}")
		end
	else
		error("cannot encode value of type " .. kind)
	end
end

function Json.encode(value: any): string
	local out: {string} = {}
	encodeValue(value, out)
	return table.concat(out)
end

local function fail(text: string, pos: number, what: string): never
	error(string.format("json: %s at byte %d", what, pos), 0)
end

local function skip(text: string, pos: number): number
	local _, e = text:find("^[ \t\r\n]*", pos)
	return (e :: number) + 1
end

local SIMPLE: {[string]: string} = {
	['"'] = '"',
	["\\"] = "\\",
	["/"] = "/",
	b = "\b",
	f = "\f",
	n = "\n",
	r = "\r",
	t = "\t",
}

local function parseHex4(text: string, pos: number): number
	local hex = text:match("^%x%x%x%x", pos)
	if not hex then
		fail(text, pos, "bad unicode escape")
	end
	return tonumber(hex, 16) :: number
end

local function parseString(text: string, pos: number): (string, number)
	local parts: {string} = {}
	local i = pos + 1
	while true do
		local s, e = text:find('^[^"\\%c]+', i)
		if s then
			table.insert(parts, text:sub(s, e))
			i = (e :: number) + 1
		end
		local c = text:sub(i, i)
		if c == '"' then
			return table.concat(parts), i + 1
		elseif c == "\\" then
			local esc = text:sub(i + 1, i + 1)
			if esc == "u" then
				local code = parseHex4(text, i + 2)
				i += 6
				if code >= 0xD800 and code <= 0xDBFF then
					if text:sub(i, i + 1) ~= "\\u" then
						fail(text, i, "lone surrogate")
					end
					local low = parseHex4(text, i + 2)
					if low < 0xDC00 or low > 0xDFFF then
						fail(text, i, "bad surrogate pair")
					end
					code = 0x10000 + (code - 0xD800) * 0x400 + (low - 0xDC00)
					i += 6
				elseif code >= 0xDC00 and code <= 0xDFFF then
					fail(text, i, "lone surrogate")
				end
				table.insert(parts, utf8.char(code))
			else
				local simple = SIMPLE[esc]
				if not simple then
					fail(text, i, "bad escape")
				end
				table.insert(parts, simple)
				i += 2
			end
		else
			fail(text, i, "unterminated or invalid string")
		end
	end
end

local parseValue

local function parseNumber(text: string, pos: number): (number, number)
	local literal = text:match("^-?%d+%.?%d*[eE]?[+-]?%d*", pos)
	local n = literal and tonumber(literal)
	if not literal or not n then
		fail(text, pos, "bad number")
	end
	return n :: number, pos + #(literal :: string)
end

function parseValue(text: string, pos: number): (any, number)
	pos = skip(text, pos)
	local c = text:sub(pos, pos)
	if c == "{" then
		local obj: {[string]: any} = {}
		pos = skip(text, pos + 1)
		if text:sub(pos, pos) == "}" then
			return obj, pos + 1
		end
		while true do
			pos = skip(text, pos)
			if text:sub(pos, pos) ~= '"' then
				fail(text, pos, "expected object key")
			end
			local key
			key, pos = parseString(text, pos)
			pos = skip(text, pos)
			if text:sub(pos, pos) ~= ":" then
				fail(text, pos, "expected ':'")
			end
			local value
			value, pos = parseValue(text, pos + 1)
			obj[key] = value
			pos = skip(text, pos)
			local d = text:sub(pos, pos)
			if d == "}" then
				return obj, pos + 1
			elseif d ~= "," then
				fail(text, pos, "expected ',' or '}'")
			end
			pos += 1
		end
	elseif c == "[" then
		local arr = Json.array()
		pos = skip(text, pos + 1)
		if text:sub(pos, pos) == "]" then
			return arr, pos + 1
		end
		while true do
			local value
			value, pos = parseValue(text, pos)
			table.insert(arr, value)
			pos = skip(text, pos)
			local d = text:sub(pos, pos)
			if d == "]" then
				return arr, pos + 1
			elseif d ~= "," then
				fail(text, pos, "expected ',' or ']'")
			end
			pos += 1
		end
	elseif c == '"' then
		return parseString(text, pos)
	elseif text:sub(pos, pos + 3) == "true" then
		return true, pos + 4
	elseif text:sub(pos, pos + 4) == "false" then
		return false, pos + 5
	elseif text:sub(pos, pos + 3) == "null" then
		return Json.null, pos + 4
	elseif c == "-" or c:match("%d") then
		return parseNumber(text, pos)
	end
	fail(text, pos, "unexpected character")
end

function Json.decode(text: string): any
	local value, pos = parseValue(text, 1)
	pos = skip(text, pos)
	if pos <= #text then
		fail(text, pos, "trailing data")
	end
	return value
end

return Json
