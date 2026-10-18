-- Applies co-edit batches from the relay log to the place, in log order,
-- inside one undo recording per batch. Shares `state` with Coedit.luau:
-- idOf/byId identity maps, LWW registers, the script text log, echo marks
-- and the set of instances whose add/remove events must be ignored.

local CoeditClasses = require(script.Parent.CoeditClasses)
local CoeditCore = require(script.Parent.CoeditCore)
local CoeditValue = require(script.Parent.CoeditValue)
local Json = require(script.Parent.Json)
local Paths = require(script.Parent.Paths)

local EXISTS = "\0exists"

local Apply = {}
Apply.__index = Apply

function Apply.valueType(className: string, prop: string): string?
	local cls = CoeditClasses[className]
	while cls ~= nil do
		local t = cls.p and cls.p[prop]
		if t ~= nil then
			return t
		end
		cls = cls.s and CoeditClasses[cls.s] or nil
	end
	if prop == "Parent" then
		return "Ref"
	elseif prop == "Name" then
		return "String"
	end
	return nil
end

-- Every scripting property of the class, inherited ones included.
function Apply.propsOf(className: string): { [string]: string }
	local out: { [string]: string } = {}
	local cls = CoeditClasses[className]
	while cls ~= nil do
		if cls.p then
			for prop, t in cls.p do
				if out[prop] == nil then
					out[prop] = t
				end
			end
		end
		cls = cls.s and CoeditClasses[cls.s] or nil
	end
	return out
end

function Apply.targetOf(state: any, inst: Instance): { id: string, p: string }
	return { id = state.idOf[inst] or "", p = Paths.pathOf(inst) or "" }
end

function Apply.resolve(state: any, target: any): Instance?
	if type(target) ~= "table" then
		return nil
	end
	local id = type(target.id) == "string" and target.id or ""
	local inst = if id ~= "" then state.byId[id] else nil
	if inst == nil and type(target.p) == "string" and target.p ~= "" then
		inst = Paths.resolve(game, target.p)
	end
	if inst ~= nil and id ~= "" then
		state.idOf[inst] = id
		state.byId[id] = inst
	end
	return inst
end

function Apply.keyOf(target: any): string
	local id = type(target.id) == "string" and target.id or ""
	if id ~= "" then
		return id
	end
	return "p:" .. tostring(target.p or "")
end

-- The property's current value as a JSON string, the form echo marks hold.
function Apply.encodeProp(state: any, inst: Instance, prop: string): string?
	local ok, value = pcall(function()
		return (inst :: any)[prop]
	end)
	if not ok then
		return nil
	end
	local encoded = CoeditValue.encode(value, Apply.valueType(inst.ClassName, prop), function(i: Instance)
		return Apply.targetOf(state, i)
	end)
	if encoded == nil then
		return nil
	end
	local okJson, text = pcall(Json.encode, encoded)
	return if okJson then text else nil
end

function Apply.mark(state: any, inst: Instance, prop: string, encoded: string)
	local marks = state.marks[inst]
	if marks == nil then
		marks = {}
		state.marks[inst] = marks
	end
	marks[prop] = encoded
end

function Apply.editorSource(inst: Instance): string
	local ok, text = pcall(function()
		return game:GetService("ScriptEditorService"):GetEditorSource(inst)
	end)
	if ok and type(text) == "string" then
		return text
	end
	local okSource, source = pcall(function()
		return (inst :: any).Source
	end)
	return if okSource and type(source) == "string" then source else ""
end

function Apply.writeSource(state: any, inst: Instance, text: string)
	Apply.mark(state, inst, "Source", text)
	local ok = pcall(function()
		game:GetService("ScriptEditorService"):UpdateSourceAsync(inst, function()
			return text
		end)
	end)
	if not ok then
		pcall(function()
			(inst :: any).Source = text
		end)
	end
end

function Apply.new(state: any)
	return setmetatable({ state = state }, Apply)
end

local function writeProp(state: any, inst: Instance, prop: string, json: any, notes: { string }): boolean
	local current: any = nil
	pcall(function()
		current = (inst :: any)[prop]
	end)
	local ok, value = CoeditValue.decode(json, current, Apply.valueType(inst.ClassName, prop), function(t)
		return Apply.resolve(state, t)
	end)
	if not ok then
		table.insert(notes, string.format("Co-edit: could not read %s.%s", inst:GetFullName(), prop))
		return false
	end
	local okSet = pcall(function()
		(inst :: any)[prop] = value
	end)
	if not okSet then
		return false
	end
	local encoded = Apply.encodeProp(state, inst, prop)
	if encoded ~= nil then
		Apply.mark(state, inst, prop, encoded)
	end
	return true
end

local function ignoreSubtree(state: any, inst: Instance)
	state.ignore[inst] = true
	for _, d in inst:GetDescendants() do
		state.ignore[d] = true
	end
end

function Apply.batch(self: any, batch: any, own: boolean, authorName: string)
	local state = self.state
	local notes: { string } = {}
	local resend: { any } = {}
	local applied = 0
	local history = game:GetService("ChangeHistoryService")
	local okRec, recording = pcall(function()
		return history:TryBeginRecording("Co-edit", "Co-edit from " .. authorName)
	end)
	if not okRec then
		recording = nil
	end

	for _, op in (type(batch.ops) == "table" and batch.ops or {}) do
		if type(op) ~= "table" or type(op.c) ~= "table" or type(op.c[1]) ~= "number" then
			continue
		end
		state.counter = math.max(state.counter or 0, op.c[1])
		local target = { id = op.id or "", p = op.p or "" }
		local key = Apply.keyOf(target)
		local inst = Apply.resolve(state, target)
		if op.k == "set" and type(op.n) == "string" then
			if state.registers:claim(key .. "\0" .. op.n, op.c) and not own then
				if inst == nil then
					table.insert(notes, "Co-edit: no instance at " .. tostring(op.p))
				elseif op.v ~= nil and writeProp(state, inst, op.n, op.v, notes) then
					applied += 1
				end
			end
		elseif op.k == "add" and type(op.cls) == "string" then
			if state.registers:claim(key .. "\0" .. EXISTS, op.c) and not own then
				local exists = target.id ~= "" and state.byId[target.id] ~= nil
				local parent = if type(op.parent) == "table" then Apply.resolve(state, op.parent) else nil
				if not exists and parent ~= nil then
					local okNew, created = pcall(Instance.new, op.cls)
					if okNew then
						state.ignore[created] = true
						if target.id ~= "" then
							state.idOf[created] = target.id
							state.byId[target.id] = created
						end
						local source: string? = nil
						for prop, value in (type(op.props) == "table" and op.props or {}) do
							if prop == "Parent" then
								continue
							elseif prop == "Source" and type(value) == "table" and type(value.v) == "string" then
								source = value.v
							end
							writeProp(state, created, prop, value, notes)
						end
						if source ~= nil then
							state.log:hash(key, source)
						end
						local okParent = pcall(function()
							created.Parent = parent
						end)
						if okParent then
							applied += 1
						else
							state.ignore[created] = nil
							created:Destroy()
							table.insert(notes, "Co-edit: could not add " .. op.cls .. " at " .. tostring(op.p))
						end
					else
						table.insert(notes, "Co-edit: Studio cannot create " .. op.cls)
					end
				elseif not exists then
					table.insert(notes, "Co-edit: no parent for " .. tostring(op.p))
				end
			end
		elseif op.k == "del" then
			if state.registers:claim(key .. "\0" .. EXISTS, op.c) and not own and inst ~= nil then
				ignoreSubtree(state, inst)
				if pcall(inst.Destroy, inst) then
					applied += 1
				end
			end
		elseif op.k == "src" and type(op.text) == "string" and type(op.base) == "string" then
			if inst ~= nil and inst:IsA("LuaSourceContainer") then
				local current = Apply.editorSource(inst)
				local result = state.log:src(key, current, op, own)
				if result.write ~= nil then
					Apply.writeSource(state, inst, result.write)
					applied += 1
				end
				if result.conflicted then
					table.insert(notes, "Co-edit: conflicting edits merged with markers in " .. inst:GetFullName())
				end
				if result.resend ~= nil then
					local logged = state.log.texts[key] or current
					table.insert(resend, {
						k = "src",
						id = target.id,
						p = target.p,
						base = CoeditCore.textHash(logged),
						text = result.resend,
					})
				end
			elseif inst == nil then
				table.insert(notes, "Co-edit: no script at " .. tostring(op.p))
			end
		end
	end

	if recording ~= nil then
		pcall(function()
			history:FinishRecording(
				recording,
				if applied > 0 then Enum.FinishRecordingOperation.Commit else Enum.FinishRecordingOperation.Cancel
			)
		end)
	end
	return { applied = applied, notes = notes, resend = resend }
end

return Apply
