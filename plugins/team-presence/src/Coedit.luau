-- Live co-editing in Studio: captures local edits under the tracked roots,
-- sends them to the relay op log as sealed chunks, and applies every batch of
-- the log (our own included) in log order through CoeditApply.

local Apply = require(script.Parent.CoeditApply)
local CoeditCore = require(script.Parent.CoeditCore)
local Crypto = require(script.Parent.Crypto)
local Json = require(script.Parent.Json)
local Paths = require(script.Parent.Paths)

local FLUSH_EVERY = 0.3
local POLL_EVERY = 0.5
local POST_TRIES = 5

local ROOTS = {
	"Workspace",
	"Lighting",
	"ReplicatedFirst",
	"ReplicatedStorage",
	"ServerScriptService",
	"ServerStorage",
	"StarterGui",
	"StarterPack",
	"StarterPlayer",
	"SoundService",
	"Teams",
	"Chat",
	"TextChatService",
	"MaterialService",
}

export type Options = {
	relay: any,
	userId: number,
	room: string,
	encKey: string,
	name: string,
	onNote: (string) -> (),
}

local rng = Random.new()

-- Plugins have no CSPRNG; a v4 GUID mixed with a Random stream gives a nonce
-- that does not repeat.
local function nonceFor(): string
	local hex = string.gsub(game:GetService("HttpService"):GenerateGUID(false), "[^%x]", "")
	local bytes = table.create(12)
	for i = 1, 12 do
		local guidByte = tonumber(string.sub(hex, i * 2 - 1, i * 2), 16) or 0
		bytes[i] = string.char(bit32.bxor(guidByte, rng:NextInteger(0, 255)))
	end
	return table.concat(bytes)
end

local function freshId(): string
	return string.lower((string.gsub(game:GetService("HttpService"):GenerateGUID(false), "-", "")))
end

local Coedit = {}
Coedit.__index = Coedit

function Coedit.new(options: Options)
	local actor = string.format("%.0f", options.userId) .. ":studio:" .. CoeditCore.session()
	local state = {
		idOf = setmetatable({}, { __mode = "k" }),
		byId = setmetatable({}, { __mode = "v" }),
		registers = CoeditCore.Registers.new(),
		log = CoeditCore.Log.new(),
		marks = setmetatable({}, { __mode = "k" }),
		ignore = setmetatable({}, { __mode = "k" }),
		counter = 0,
	}
	return setmetatable({
		options = options,
		actor = actor,
		state = state,
		apply = Apply.new(state),
		running = false,
		connections = {} :: { RBXScriptConnection },
		watched = {} :: { [Instance]: RBXScriptConnection },
		roots = {} :: { [Instance]: boolean },
		secret = nil :: Instance?,
		added = {} :: { [Instance]: boolean },
		dirty = {} :: { [Instance]: { [string]: boolean } },
		dirtySrc = {} :: { [Instance]: boolean },
		removed = {} :: { { inst: Instance, id: string, p: string } },
		resend = {} :: { any },
		paths = setmetatable({}, { __mode = "k" }),
		since = 0,
		batchCount = 0,
	}, Coedit)
end

function Coedit.note(self: any, text: string)
	pcall(self.options.onNote, text)
end

function Coedit.isTracked(self: any, inst: Instance): boolean
	local current: Instance? = inst
	while current ~= nil do
		if current == self.secret then
			return false
		end
		if self.roots[current] then
			return current ~= inst
		end
		current = current.Parent
	end
	return false
end

function Coedit.scriptKey(self: any, inst: Instance): string
	return Apply.keyOf(Apply.targetOf(self.state, inst))
end

function Coedit.onChanged(self: any, inst: Instance, prop: string)
	if not self.running or self.state.ignore[inst] then
		return
	end
	if prop == "Name" or prop == "Parent" then
		if inst.Parent ~= nil then
			self.paths[inst] = Paths.pathOf(inst)
		end
	end
	if prop == "Source" then
		self.dirtySrc[inst] = true
		return
	end
	if prop ~= "Name" and prop ~= "Parent" and Apply.valueType(inst.ClassName, prop) == nil then
		return
	end
	local marks = self.state.marks[inst]
	if marks ~= nil and marks[prop] ~= nil then
		local mark = marks[prop]
		marks[prop] = nil
		if Apply.encodeProp(self.state, inst, prop) == mark then
			return
		end
	end
	if prop == "Parent" and (inst.Parent == nil or not self:isTracked(inst)) then
		return
	end
	local props = self.dirty[inst]
	if props == nil then
		props = {}
		self.dirty[inst] = props
	end
	props[prop] = true
end

function Coedit.watch(self: any, inst: Instance)
	if self.watched[inst] ~= nil or not self:isTracked(inst) then
		return
	end
	self.watched[inst] = inst.Changed:Connect(function(prop: string)
		self:onChanged(inst, prop)
	end)
	self.paths[inst] = Paths.pathOf(inst)
	if inst:IsA("LuaSourceContainer") then
		self.state.log:hash(self:scriptKey(inst), Apply.editorSource(inst))
	end
end

function Coedit.unwatch(self: any, inst: Instance)
	local conn = self.watched[inst]
	if conn ~= nil then
		conn:Disconnect()
		self.watched[inst] = nil
	end
end

function Coedit.onAdded(self: any, inst: Instance)
	if self.state.ignore[inst] then
		self.state.ignore[inst] = nil
		self:watch(inst)
		return
	end
	if not self:isTracked(inst) then
		return
	end
	self:watch(inst)
	self.added[inst] = true
end

function Coedit.onRemoving(self: any, inst: Instance)
	if self.state.ignore[inst] then
		self.state.ignore[inst] = nil
		self:unwatch(inst)
		return
	end
	if self.watched[inst] == nil then
		return
	end
	if self.added[inst] then
		self.added[inst] = nil
	else
		table.insert(self.removed, {
			inst = inst,
			id = self.state.idOf[inst] or "",
			p = self.paths[inst] or Paths.pathOf(inst) or "",
		})
	end
	self.dirty[inst] = nil
	self.dirtySrc[inst] = nil
	self:unwatch(inst)
end

function Coedit.onDocumentChanged(self: any, document: any)
	local ok, inst = pcall(function()
		if document:IsCommandBar() then
			return nil
		end
		return document:GetScript()
	end)
	if ok and inst ~= nil and self.watched[inst] ~= nil then
		self.dirtySrc[inst] = true
	end
end

function Coedit.clock(self: any): { any }
	self.state.counter += 1
	return { self.state.counter, self.actor }
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

function Coedit.collect(self: any): { any }
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
		for prop in Apply.propsOf(inst.ClassName) do
			if prop ~= "Parent" then
				local okEnc, encoded = pcall(Apply.encodeProp, state, inst, prop)
				if okEnc and encoded ~= nil then
					props[prop] = Json.decode(encoded)
				end
			end
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
		for prop in props do
			local encoded = Apply.encodeProp(state, inst, prop)
			if encoded ~= nil then
				table.insert(ops, { k = "set", id = at.id, p = at.p, n = prop, v = Json.decode(encoded), c = self:clock() })
			end
		end
	end
	self.dirty = {}

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

function Coedit.send(self: any, ops: { any }): boolean
	local options = self.options
	local okJson, batchJson = pcall(Json.encode, { a = self.actor, ops = ops })
	if not okJson then
		self:note("Co-edit: could not encode an edit batch")
		return false
	end
	self.batchCount += 1
	local id = CoeditCore.session() .. string.format("%x", self.batchCount)
	local aad = Crypto.opsAad(options.room, options.userId, "studio")
	for _, chunk in CoeditCore.split(id, batchJson) do
		local sealed = Crypto.sealB64(options.encKey, nonceFor(), aad, Json.encode(chunk))
		local body = Json.encode({ v = 2, userId = options.userId, client = "studio", ct = sealed })
		local sent = false
		local lastError: string? = nil
		for attempt = 1, POST_TRIES do
			if not self.running then
				return false
			end
			local okPost, ok, err = pcall(options.relay.postOps, options.relay, body)
			if okPost and ok then
				sent = true
				break
			end
			lastError = if okPost then err else tostring(ok)
			task.wait(0.5 * 2 ^ (attempt - 1))
		end
		if not sent then
			self:note("Co-edit: could not send edits (" .. tostring(lastError) .. ")")
			return false
		end
	end
	return true
end

function Coedit.flush(self: any)
	local ops = self:collect()
	if #ops > 0 then
		self:send(ops)
	end
end

function Coedit.authorOf(_self: any, actor: any): string
	if type(actor) ~= "string" then
		return "a collaborator"
	end
	local userId, client = string.match(actor, "^(%d+):([^:]+)")
	if userId == nil then
		return actor
	end
	return "user " .. userId .. " (" .. client .. ")"
end

function Coedit.poll(self: any)
	local options = self.options
	local okPoll, result, err = pcall(options.relay.pollOps, options.relay, self.since)
	if not okPoll or result == nil then
		if not okPoll or err ~= nil then
			self:note("Co-edit: could not read edits (" .. tostring(if okPoll then err else result) .. ")")
		end
		return
	end
	local entries = type(result.entries) == "table" and result.entries or {}
	table.sort(entries, function(a, b)
		return (a.seq or 0) < (b.seq or 0)
	end)
	if type(result.floor) == "number" and result.floor > self.since and self.since > 0 then
		self:note("Co-edit: the relay dropped older edits; some changes may be missing")
	end
	for _, entry in entries do
		if type(entry) ~= "table" or type(entry.seq) ~= "number" or type(entry.ct) ~= "string" then
			continue
		end
		local client = tostring(entry.client)
		local userId = tonumber(entry.userId) or 0
		local plain = Crypto.openB64(options.encKey, Crypto.opsAad(options.room, userId, client), entry.ct)
		if plain == nil then
			continue
		end
		local okChunk, chunk = pcall(Json.decode, plain)
		if not okChunk or type(chunk) ~= "table" then
			continue
		end
		local text = self.reassembler:push(entry.seq, string.format("%.0f", userId) .. ":" .. client, chunk)
		if text == nil then
			continue
		end
		local okBatch, batch = pcall(Json.decode, text)
		if okBatch and type(batch) == "table" then
			local outcome = self.apply:batch(batch, batch.a == self.actor, self:authorOf(batch.a))
			for _, line in outcome.notes do
				self:note(line)
			end
			for _, op in outcome.resend do
				table.insert(self.resend, op)
			end
		end
	end
	if type(result.head) == "number" then
		self.since = math.max(self.since, result.head)
	end
end

function Coedit.start(self: any)
	if self.running then
		return
	end
	self.running = true
	self.reassembler = CoeditCore.Reassembler.new()
	local ok, secret = pcall(function()
		return game:GetService("ServerStorage"):FindFirstChild("RbxNativePresence")
	end)
	self.secret = if ok then secret else nil
	for _, name in ROOTS do
		local okRoot, root = pcall(function()
			return game:GetService(name)
		end)
		if okRoot and root ~= nil then
			self.roots[root] = true
		end
	end
	for root in self.roots do
		table.insert(self.connections, root.DescendantAdded:Connect(function(d)
			if self.running then
				self:onAdded(d)
			end
		end))
		table.insert(self.connections, root.DescendantRemoving:Connect(function(d)
			if self.running then
				self:onRemoving(d)
			end
		end))
		for _, d in root:GetDescendants() do
			self:watch(d)
		end
	end
	if self.roots[game:GetService("ServerStorage")] then
		table.insert(self.connections, game:GetService("ServerStorage").ChildAdded:Connect(function(child)
			if child.Name == "RbxNativePresence" and self.secret == nil then
				self.secret = child
				self:unwatch(child)
				self.added[child] = nil
			end
		end))
	end
	pcall(function()
		local editor = game:GetService("ScriptEditorService")
		table.insert(self.connections, editor.TextDocumentDidChange:Connect(function(document)
			if self.running then
				self:onDocumentChanged(document)
			end
		end))
	end)
	task.spawn(function()
		while self.running do
			task.wait(FLUSH_EVERY)
			if self.running then
				local okFlush, flushErr = pcall(self.flush, self)
				if not okFlush then
					self:note("Co-edit: " .. tostring(flushErr))
				end
			end
		end
	end)
	task.spawn(function()
		while self.running do
			local okPoll, pollErr = pcall(self.poll, self)
			if not okPoll then
				self:note("Co-edit: " .. tostring(pollErr))
			end
			task.wait(POLL_EVERY)
		end
	end)
end

function Coedit.stop(self: any)
	self.running = false
	for _, conn in self.connections do
		conn:Disconnect()
	end
	self.connections = {}
	for _, conn in self.watched do
		conn:Disconnect()
	end
	self.watched = {}
	self.roots = {}
	self.added = {}
	self.dirty = {}
	self.dirtySrc = {}
	self.removed = {}
end

return Coedit
