-- Live co-editing in Studio: captures local edits under the tracked roots
-- (CoeditCollect turns them into ops), sends them to the relay op log and
-- applies every batch of the log, our own included, in log order through
-- CoeditApply (CoeditSync does the traffic).

local Apply = require(script.Parent.CoeditApply)
local Collect = require(script.Parent.CoeditCollect)
local CoeditCore = require(script.Parent.CoeditCore)
local Kept = require(script.Parent.CoeditKept)
local Paths = require(script.Parent.Paths)
local Sync = require(script.Parent.CoeditSync)

local FLUSH_EVERY = 0.3
local POLL_EVERY = 0.5

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

local Coedit = {}
Coedit.__index = Coedit
Coedit.collect = Collect.collect
Coedit.send = Sync.send
Coedit.flush = Sync.flush
Coedit.authorOf = Sync.authorOf
Coedit.poll = Sync.poll

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
		watched = {} :: { [Instance]: { RBXScriptConnection } },
		tagsHooked = {} :: { [string]: boolean },
		roots = {} :: { [Instance]: boolean },
		secret = nil :: Instance?,
		added = {} :: { [Instance]: boolean },
		dirty = {} :: { [Instance]: { [string]: boolean } },
		dirtySrc = {} :: { [Instance]: boolean },
		removed = {} :: { { inst: Instance, id: string, p: string } },
		resend = {} :: { any },
		paths = setmetatable({}, { __mode = "k" }),
		moved = setmetatable({}, { __mode = "k" }),
		since = 0,
		kept = Kept.new(),
		batchCount = 0,
	}, Coedit)
end

function Coedit.note(self: any, text: string)
	pcall(self.options.onNote, text)
end

function Coedit.isTracked(self: any, inst: Instance): boolean
	local current: Instance? = inst
	while current ~= nil do
		-- The camera is each editor's own view, and holds the presence
		-- markers: none of it is shared.
		if current == self.secret or current:IsA("Camera") then
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
		-- Peers find an instance whose id they don't know by its path, so a
		-- rename or move is sent at the path it had before.
		if self.moved[inst] == nil then
			self.moved[inst] = self.paths[inst]
		end
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
	self.watched[inst] = {
		inst.Changed:Connect(function(prop: string)
			self:onChanged(inst, prop)
		end),
		inst.AttributeChanged:Connect(function(name: string)
			self:onChanged(inst, "@" .. name)
		end),
	}
	self.paths[inst] = Paths.pathOf(inst)
	if inst:IsA("LuaSourceContainer") then
		self.state.log:hash(self:scriptKey(inst), Apply.editorSource(inst))
	end
end

function Coedit.unwatch(self: any, inst: Instance)
	local conns = self.watched[inst]
	if conns ~= nil then
		for _, conn in conns do
			conn:Disconnect()
		end
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
	local tags = game:GetService("CollectionService")
	local function hookTag(tag: string)
		if self.tagsHooked[tag] then
			return nil
		end
		self.tagsHooked[tag] = true
		local function changed(inst: Instance)
			if self.running and self.watched[inst] ~= nil then
				self:onChanged(inst, "#" .. tag)
			end
		end
		table.insert(self.connections, tags:GetInstanceAddedSignal(tag):Connect(changed))
		table.insert(self.connections, tags:GetInstanceRemovedSignal(tag):Connect(changed))
		return changed
	end
	for _, tag in tags:GetAllTags() do
		hookTag(tag)
	end
	-- A tag's first use fires TagAdded only after the instance has it, so
	-- the instances already tagged are picked up here.
	table.insert(self.connections, tags.TagAdded:Connect(function(tag: string)
		local changed = hookTag(tag)
		if changed ~= nil then
			for _, inst in tags:GetTagged(tag) do
				changed(inst)
			end
		end
	end))
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
	for _, conns in self.watched do
		for _, conn in conns do
			conn:Disconnect()
		end
	end
	self.watched = {}
	self.tagsHooked = {}
	self.roots = {}
	self.added = {}
	self.dirty = {}
	self.dirtySrc = {}
	self.removed = {}
end

return Coedit
