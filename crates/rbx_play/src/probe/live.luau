-- Live tree (ModuleScript): one side's instance tree, diffs, watched
-- properties and edits, as rbx_play::protocol messages handed to `send`.
local HttpService = game:GetService("HttpService")

local ROOT_NAMES = {
	"Workspace",
	"Players",
	"Lighting",
	"MaterialService",
	"ReplicatedFirst",
	"ReplicatedStorage",
	"ServerScriptService",
	"ServerStorage",
	"StarterGui",
	"StarterPack",
	"StarterPlayer",
	"Teams",
	"SoundService",
	"Chat",
	"TextChatService",
	"LocalizationService",
	"TestService",
}
local PREFIX = "__rbxnative_"
local CHUNK = 1000
local PERIOD = 0.25
local MAX_TRACKED = 50000

local Live = {}

local function jsonKey(value)
	local ok, s = pcall(HttpService.JSONEncode, HttpService, { value })
	if ok then
		return s
	end
	return tostring(value)
end

function Live.new(side, send, codec)
	local self = {}
	local idOf = setmetatable({}, { __mode = "k" })
	local byId = setmetatable({}, { __mode = "v" })
	local conns = setmetatable({}, { __mode = "k" })
	local connCount = 0
	local nextId = 1
	local roots = {}
	local rootSet = {}
	local running = true

	local added = {}
	local removing = {}
	local renamed = {}
	local moved = {}

	local watchItems = {}

	codec.setIdResolver(function(inst)
		return idOf[inst] or 0
	end, function(id)
		return byId[id]
	end)

	for _, name in ROOT_NAMES do
		local ok, service = pcall(game.GetService, game, name)
		if ok and service then
			table.insert(roots, service)
			rootSet[service] = true
		end
	end

	-- Inside a tracked root and not under a __rbxnative_ instance.
	local function inTree(inst)
		local a = inst
		while a do
			if string.sub(a.Name, 1, #PREFIX) == PREFIX then
				return false
			end
			if rootSet[a] then
				return true
			end
			a = a.Parent
		end
		return false
	end

	local function assign(inst)
		local id = idOf[inst]
		if id then
			return id
		end
		id = nextId
		nextId += 1
		idOf[inst] = id
		byId[id] = inst
		return id
	end

	local function forget(inst)
		local id = idOf[inst]
		if id then
			byId[id] = nil
			idOf[inst] = nil
		end
		local c = conns[inst]
		if c then
			for _, conn in c do
				conn:Disconnect()
			end
			conns[inst] = nil
			connCount -= 1
		end
	end

	local function track(inst)
		if conns[inst] or connCount >= MAX_TRACKED then
			return
		end
		connCount += 1
		conns[inst] = {
			inst:GetPropertyChangedSignal("Name"):Connect(function()
				renamed[inst] = true
			end),
			inst:GetPropertyChangedSignal("Parent"):Connect(function()
				moved[inst] = true
			end),
		}
	end

	local function node(inst)
		local parentId = 0
		if not rootSet[inst] then
			parentId = idOf[inst.Parent] or 0
		end
		return { id = assign(inst), parent = parentId, class = inst.ClassName, name = inst.Name }
	end

	local function flush()
		-- Uplink::Diff; wire tags are lowercase like Tree ("tree") and Props ("props").
		local out = { type = "diff", side = side, added = {}, removed = {}, renamed = {}, moved = {} }
		local fresh = {}

		local function addNode(inst)
			if inst == nil or idOf[inst] then
				return
			end
			if not rootSet[inst] then
				addNode(inst.Parent)
			end
			local n = node(inst)
			fresh[inst] = true
			track(inst)
			table.insert(out.added, n)
		end

		for inst in removing do
			if idOf[inst] and inTree(inst) then
				moved[inst] = true
			end
		end
		for inst in added do
			if inst.Parent and inTree(inst) then
				if idOf[inst] then
					moved[inst] = true
				else
					addNode(inst)
				end
			end
		end
		for inst in removing do
			local id = idOf[inst]
			if id and not inTree(inst) then
				table.insert(out.removed, id)
				forget(inst)
			end
		end
		for inst in renamed do
			local id = idOf[inst]
			if id and not fresh[inst] then
				table.insert(out.renamed, { id = id, name = inst.Name })
			end
		end
		for inst in moved do
			local id = idOf[inst]
			if id and not fresh[inst] and inst.Parent and inTree(inst) then
				table.insert(out.moved, { id = id, parent = idOf[inst.Parent] or 0 })
			end
		end
		added, removing, renamed, moved = {}, {}, {}, {}

		if #out.added + #out.removed + #out.renamed + #out.moved > 0 then
			send(out)
		end
	end

	for _, root in roots do
		root.DescendantAdded:Connect(function(inst)
			added[inst] = true
		end)
		root.DescendantRemoving:Connect(function(inst)
			if idOf[inst] then
				removing[inst] = true
			end
		end)
	end

	function self.snapshot()
		added, removing, renamed, moved = {}, {}, {}, {}
		local queue = {}
		for _, root in roots do
			table.insert(queue, root)
		end
		local chunk = {}
		local first = true
		local head = 1
		while head <= #queue do
			local inst = queue[head]
			queue[head] = false
			head += 1
			if rootSet[inst] or (inst.Parent and inTree(inst)) then
				track(inst)
				table.insert(chunk, node(inst))
				for _, child in inst:GetChildren() do
					if string.sub(child.Name, 1, #PREFIX) ~= PREFIX then
						table.insert(queue, child)
					end
				end
			end
			if #chunk >= CHUNK and head <= #queue then
				send({ type = "tree", side = side, reset = first, done = false, items = chunk })
				first = false
				chunk = {}
				task.wait()
			end
		end
		send({ type = "tree", side = side, reset = first, done = true, items = chunk })
	end

	local function readWatched(state)
		local inst = byId[state.id]
		if inst == nil then
			return
		end
		local values = {}
		local any = false
		for _, prop in state.props do
			local ok, value = pcall(function()
				return inst[prop]
			end)
			if ok then
				local encoded = codec.encode(value)
				if encoded == nil then
					encoded = { t = "I", v = 0 }
				end
				local key = jsonKey(encoded)
				if state.last[prop] ~= key then
					state.last[prop] = key
					values[prop] = encoded
					any = true
				end
			end
		end
		local message = { type = "props", side = side, id = state.id, values = values }

		local okA, attrs = pcall(inst.GetAttributes, inst)
		if okA then
			local enc = {}
			for name, value in attrs do
				enc[name] = codec.encode(value)
			end
			local key = jsonKey(enc)
			if state.attrs ~= key then
				state.attrs = key
				message.attributes = enc
				any = true
			end
		end
		local okT, tags = pcall(inst.GetTags, inst)
		if okT then
			local key = jsonKey(tags)
			if state.tags ~= key then
				state.tags = key
				message.tags = tags
				any = true
			end
		end
		if any then
			send(message)
		end
	end

	function self.watch(items)
		local states = {}
		for _, item in items or {} do
			table.insert(states, { id = item.id, props = item.props or {}, last = {} })
		end
		watchItems = states
		for _, state in states do
			readWatched(state)
		end
		return true, "watching " .. #states
	end

	local function target(id)
		local inst = byId[id]
		if inst == nil then
			error("instance " .. tostring(id) .. " is not in the " .. side .. " tree", 0)
		end
		return inst
	end

	local function guarded(f)
		local ok, result = pcall(f)
		if ok then
			return true, result or "ok"
		end
		return false, tostring(result)
	end

	function self.set(targetId, prop, value)
		return guarded(function()
			local inst = target(targetId)
			local okCur, current = pcall(function()
				return inst[prop]
			end)
			local decoded, err = codec.decode(value, okCur and current or nil)
			if err then
				error(err, 0)
			end
			inst[prop] = decoded
			return prop .. " set"
		end)
	end

	function self.set_attribute(targetId, name, value)
		return guarded(function()
			local inst = target(targetId)
			local decoded, err = codec.decode(value, inst:GetAttribute(name))
			if err then
				error(err, 0)
			end
			inst:SetAttribute(name, decoded)
			return "attribute " .. name .. " set"
		end)
	end

	function self.set_tags(targetId, tags)
		return guarded(function()
			local inst = target(targetId)
			local wanted = {}
			for _, t in tags do
				wanted[t] = true
			end
			local have = {}
			for _, t in inst:GetTags() do
				have[t] = true
				if not wanted[t] then
					inst:RemoveTag(t)
				end
			end
			for t in wanted do
				if not have[t] then
					inst:AddTag(t)
				end
			end
			return "tags set"
		end)
	end

	function self.destroy(targetId)
		return guarded(function()
			target(targetId):Destroy()
			return "destroyed"
		end)
	end

	function self.reparent(targetId, parentId)
		return guarded(function()
			local inst = target(targetId)
			if parentId == nil or parentId == 0 then
				inst.Parent = nil
			else
				inst.Parent = target(parentId)
			end
			return "reparented"
		end)
	end

	function self.create(class, name, parentId)
		return guarded(function()
			local parent = target(parentId)
			local inst = Instance.new(class)
			inst.Name = name
			inst.Parent = parent
			return "created"
		end)
	end

	function self.resync()
		task.spawn(self.snapshot)
		return true, "resyncing"
	end

	function self.stop()
		running = false
	end

	task.spawn(function()
		while running do
			task.wait(PERIOD)
			local ok, err = pcall(flush)
			if not ok then
				warn("[rbxnative] diff flush failed: " .. tostring(err))
			end
			for _, state in watchItems do
				pcall(readWatched, state)
			end
		end
	end)

	return self
end

return Live
