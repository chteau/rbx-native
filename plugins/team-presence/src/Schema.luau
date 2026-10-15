--!strict
--! UserState / Snapshot shapes shared with the relay, including the size
--! limits the relay enforces so a state is never rejected for being too big.

local Json = require(script.Parent.Json)

local Schema = {}

Schema.VERSION = 1
Schema.MAX_SELECTION = 100
Schema.MAX_EDITS = 20
Schema.MAX_TEXT = 512
Schema.PALETTE = { "#e8590c", "#1c7ed6", "#2f9e44", "#ae3ec9", "#f08c00", "#0c8599", "#e03131", "#7048e8" }

function Schema.colorFor(userId: number): string
	return Schema.PALETTE[userId % 8 + 1]
end

local function clampText(s: string): string
	if #s <= Schema.MAX_TEXT then
		return s
	end
	local cut = Schema.MAX_TEXT
	-- Step back so a multi-byte character is never split.
	while cut > 0 do
		local b = string.byte(s, cut + 1)
		if b == nil or b < 0x80 or b >= 0xC0 then
			break
		end
		cut -= 1
	end
	return s:sub(1, cut)
end

local function clampList(list: {any}?, max: number, map: (any) -> any): {any}
	local out = Json.array()
	if list then
		for i = 1, math.min(#list, max) do
			table.insert(out, map(list[i]))
		end
	end
	return out
end

function Schema.encode(state: any): string
	local out: {[string]: any} = {
		v = Schema.VERSION,
		userId = state.userId,
		name = clampText(state.name),
		color = state.color,
		client = state.client,
		selection = clampList(state.selection, Schema.MAX_SELECTION, clampText),
		edits = clampList(state.edits, Schema.MAX_EDITS, function(edit: any): any
			local value = edit.value
			if type(value) == "string" then
				value = clampText(value)
			end
			return {
				path = clampText(edit.path),
				property = clampText(edit.property),
				value = value,
				at = edit.at,
			}
		end),
		seen = state.seen or 0,
	}
	if state.camera ~= nil then
		out.camera = Json.array(table.clone(state.camera))
	end
	if state.script ~= nil then
		out.script = clampText(state.script)
	end
	return Json.encode(out)
end

local function present(value: any): any
	if value == Json.null then
		return nil
	end
	return value
end

local function toState(obj: any): any
	if type(obj) ~= "table" then
		error("state must be an object")
	end
	local state: {[string]: any} = {
		v = obj.v or Schema.VERSION,
		userId = obj.userId,
		name = obj.name,
		color = obj.color,
		client = obj.client,
		selection = Json.array(),
		edits = Json.array(),
		seen = obj.seen or 0,
	}
	local selection = present(obj.selection)
	if selection then
		for _, path in ipairs(selection) do
			table.insert(state.selection, path)
		end
	end
	local edits = present(obj.edits)
	if edits then
		for _, edit in ipairs(edits) do
			table.insert(state.edits, edit)
		end
	end
	state.camera = present(obj.camera)
	state.script = present(obj.script)
	return state
end

function Schema.decodeState(text: string): any
	return toState(Json.decode(text))
end

function Schema.decodeSnapshot(text: string): any
	local obj = Json.decode(text)
	if type(obj) ~= "table" then
		error("snapshot must be an object")
	end
	local users: {any} = {}
	local list = present(obj.users)
	if list then
		for _, user in ipairs(list) do
			table.insert(users, toState(user))
		end
	end
	return { cursor = obj.cursor or 0, users = users }
end

return Schema
