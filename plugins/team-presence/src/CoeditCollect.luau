-- Turns what changed in Studio since the last flush into ops: deletes,
-- adds (only what differs from a fresh instance of the class), property
-- sets (renames and moves at the path peers still know) and script text.

local Apply = require(script.Parent.CoeditApply)
local Json = require(script.Parent.Json)
local Paths = require(script.Parent.Paths)

local Collect = {}

local function freshId(): string
	return string.lower((string.gsub(game:GetService("HttpService"):GenerateGUID(false), "-", "")))
end

local function depth(inst: Instance): number
	local n = 0
	local current = inst.Parent
	while current ~= nil do
		n += 1
		current = current.Parent
	end
	return n
end

-- Each class's scripting properties as a fresh `Instance.new` has them
-- (encoded), so an add only carries what differs: a part is a handful of
-- properties instead of a hundred, which matters when thousands are added.
local defaults: { [string]: { [string]: string } } = {}

local function defaultsOf(state: any, className: string): { [string]: string }
	local found = defaults[className]
	if found ~= nil then
		return found
	end
	local made: { [string]: string } = {}
	local ok, fresh = pcall(Instance.new, className)
	if ok then
		for prop in Apply.propsOf(className) do
			local okEnc, encoded = pcall(Apply.encodeProp, state, fresh, prop)
			if okEnc and encoded ~= nil then
				made[prop] = encoded
			end
		end
		fresh:Destroy()
	end
	defaults[className] = made
	return made
end

function Collect.collect(self: any): { any }
	local state = self.state
	local ops: { any } = {}
	local targetOf = function(i: Instance)
		return Apply.targetOf(state, i)
	end

	for _, entry in self.removed do
		local inst = entry.inst
		if inst.Parent ~= nil and self:isTracked(inst) then
			-- Moved between tracked roots: a reparent, not a delete.
			self:watch(inst)
			local props = self.dirty[inst] or {}
			props.Parent = true
			self.dirty[inst] = props
		else
			table.insert(ops, { k = "del", id = entry.id, p = entry.p, c = self:clock() })
		end
	end
	self.removed = {}

	local adds: { Instance } = {}
	for inst in self.added do
		if inst.Parent ~= nil and self:isTracked(inst) then
			table.insert(adds, inst)
		end
	end
	self.added = {}
	table.sort(adds, function(a, b)
		return depth(a) < depth(b)
	end)
	for _, inst in adds do
		local id = state.idOf[inst]
		if id == nil then
			id = freshId()
			state.idOf[inst] = id
			state.byId[id] = inst
		end
		local props = {}
		local default = defaultsOf(state, inst.ClassName)
		for prop in Apply.propsOf(inst.ClassName) do
			if prop ~= "Parent" then
				local okEnc, encoded = pcall(Apply.encodeProp, state, inst, prop)
				if okEnc and encoded ~= nil and encoded ~= default[prop] then
					props[prop] = Json.decode(encoded)
				end
			end
		end
		for name in inst:GetAttributes() do
			local encoded = Apply.encodeProp(state, inst, "@" .. name)
			if encoded ~= nil and encoded ~= "null" then
				props["@" .. name] = Json.decode(encoded)
			end
		end
		for _, tag in inst:GetTags() do
			props["#" .. tag] = { t = "Bool", v = true }
		end
		props.Name = { t = "String", v = inst.Name }
		if inst:IsA("LuaSourceContainer") then
			local text = Apply.editorSource(inst)
			props.Source = { t = "String", v = text }
			state.log:hash(id, text)
		end
		self.dirty[inst] = nil
		self.dirtySrc[inst] = nil
		self.paths[inst] = Paths.pathOf(inst)
		table.insert(ops, {
			k = "add",
			id = id,
			p = Paths.pathOf(inst) or "",
			cls = inst.ClassName,
			parent = targetOf(inst.Parent :: Instance),
			props = props,
			c = self:clock(),
		})
	end

	for inst, props in self.dirty do
		if inst.Parent == nil or not self:isTracked(inst) then
			continue
		end
		local at = targetOf(inst)
		local function emit(prop: string, path: string)
			local encoded = Apply.encodeProp(state, inst, prop)
			if encoded ~= nil then
				table.insert(ops, { k = "set", id = at.id, p = path, n = prop, v = Json.decode(encoded), c = self:clock() })
			end
		end
		-- Name, then Parent, each at the path the peers still have.
		local before = self.moved[inst]
		if before ~= nil then
			if props.Name then
				props.Name = nil
				emit("Name", before)
				local names = Paths.split(before)
				if names ~= nil and #names > 0 then
					names[#names] = inst.Name
					before = Paths.join(names)
				end
			end
			if props.Parent then
				props.Parent = nil
				emit("Parent", before)
			end
		end
		for prop in props do
			emit(prop, at.p)
		end
	end
	self.dirty = {}
	self.moved = setmetatable({}, { __mode = "k" })

	for inst in self.dirtySrc do
		if inst.Parent == nil or not self:isTracked(inst) then
			continue
		end
		local text = Apply.editorSource(inst)
		local marks = state.marks[inst]
		if marks ~= nil and marks.Source ~= nil then
			local mark = marks.Source
			marks.Source = nil
			if mark == text then
				continue
			end
		end
		local at = targetOf(inst)
		local key = Apply.keyOf(at)
		local logged = state.log.texts[key]
		if logged ~= nil and logged == text then
			continue
		end
		local base = state.log:hash(key, text)
		table.insert(ops, { k = "src", id = at.id, p = at.p, base = base, text = text, c = self:clock() })
	end
	self.dirtySrc = {}

	for _, op in self.resend do
		op.c = self:clock()
		table.insert(ops, op)
	end
	self.resend = {}
	return ops
end

return Collect
