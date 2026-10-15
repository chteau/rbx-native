--!strict
--! Collects the local user's presence (camera, selection, open script, recent edits) and tracks when it changed.

local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local Selection = game:GetService("Selection")
local StudioService = game:GetService("StudioService")

local Paths = require(script.Parent.Paths)
local Schema = require(script.Parent.Schema)

local SAMPLE_INTERVAL = 0.2
local MOVE_STUDS = 0.05
local TURN_RADIANS = math.rad(0.5)
local MAX_WATCHED = 50
local MERGE_MS = 2000
local EDIT_TTL_MS = 60000
local VALUE_CUT = 80

local Capture = {}
Capture.__index = Capture

local function nowMs(): number
	return DateTime.now().UnixTimestampMillis
end

function Capture.new()
	local userId = StudioService:GetUserId()
	local name = "User " .. tostring(userId)
	local ok, fetched = pcall(function()
		return Players:GetNameFromUserIdAsync(userId)
	end)
	if ok and type(fetched) == "string" and fetched ~= "" then
		name = fetched
	end

	local self = setmetatable({
		userId = userId,
		name = name,
		color = Schema.colorFor(userId),
		dirty = true,
		camera = nil :: { number }?,
		lastCFrame = nil :: CFrame?,
		lastSample = 0,
		edits = {} :: { { path: string, property: string, value: string, at: number } },
		watchers = {} :: { RBXScriptConnection },
		connections = {} :: { RBXScriptConnection },
	}, Capture)

	local function markDirty()
		self.dirty = true
	end

	table.insert(
		self.connections,
		Selection.SelectionChanged:Connect(function()
			self:_watchSelection()
			markDirty()
		end)
	)
	table.insert(self.connections, StudioService:GetPropertyChangedSignal("ActiveScript"):Connect(markDirty))
	table.insert(
		self.connections,
		RunService.Heartbeat:Connect(function()
			self:_sampleCamera()
		end)
	)
	self:_watchSelection()
	self:_sampleCamera()
	return self
end

function Capture:_sampleCamera()
	local clock = os.clock()
	if clock - self.lastSample < SAMPLE_INTERVAL then
		return
	end
	self.lastSample = clock
	local camera = workspace.CurrentCamera
	if camera == nil then
		return
	end
	local cf = camera.CFrame
	local last: CFrame? = self.lastCFrame
	local moved = last == nil
	if last ~= nil then
		local _, angle = last:ToObjectSpace(cf):ToAxisAngle()
		moved = (cf.Position - last.Position).Magnitude > MOVE_STUDS or math.abs(angle) > TURN_RADIANS
	end
	if moved then
		self.lastCFrame = cf
		self.camera = { cf:GetComponents() }
		self.dirty = true
	end
end

function Capture:_disconnectWatchers()
	for _, connection in self.watchers do
		connection:Disconnect()
	end
	table.clear(self.watchers)
end

function Capture:_recordEdit(instance: Instance, property: string)
	local path = Paths.pathOf(instance)
	if path == nil then
		return
	end
	local ok, raw = pcall(function()
		return (instance :: any)[property]
	end)
	if not ok then
		return
	end
	local value = string.sub(tostring(raw), 1, VALUE_CUT)
	local at = nowMs()
	for _, edit in self.edits do
		if edit.path == path and edit.property == property and at - edit.at <= MERGE_MS then
			edit.value = value
			edit.at = at
			self.dirty = true
			return
		end
	end
	table.insert(self.edits, { path = path, property = property, value = value, at = at })
	while #self.edits > Schema.MAX_EDITS do
		table.remove(self.edits, 1)
	end
	self.dirty = true
end

function Capture:_watchSelection()
	self:_disconnectWatchers()
	for index, instance in Selection:Get() do
		if index > MAX_WATCHED then
			break
		end
		table.insert(
			self.watchers,
			instance.Changed:Connect(function(property: string)
				self:_recordEdit(instance, property)
			end)
		)
	end
end

function Capture:_pruneEdits()
	local cutoff = nowMs() - EDIT_TTL_MS
	local kept = {}
	for _, edit in self.edits do
		if edit.at >= cutoff then
			table.insert(kept, edit)
		end
	end
	self.edits = kept
end

function Capture:state()
	local selection = {}
	for _, instance in Selection:Get() do
		if #selection >= Schema.MAX_SELECTION then
			break
		end
		local path = Paths.pathOf(instance)
		if path ~= nil then
			table.insert(selection, path)
		end
	end

	self:_pruneEdits()
	local edits = {}
	for _, edit in self.edits do
		table.insert(edits, { path = edit.path, property = edit.property, value = edit.value, at = edit.at })
	end

	local scriptPath: string? = nil
	local active = StudioService.ActiveScript
	if active ~= nil then
		scriptPath = Paths.pathOf(active)
	end

	return {
		v = Schema.VERSION,
		userId = self.userId,
		name = self.name,
		color = self.color,
		client = "studio",
		camera = self.camera,
		selection = selection,
		script = scriptPath,
		edits = edits,
		seen = 0,
	}
end

function Capture:isDirty(): boolean
	return self.dirty
end

function Capture:clearDirty()
	self.dirty = false
end

function Capture:destroy()
	self:_disconnectWatchers()
	for _, connection in self.connections do
		connection:Disconnect()
	end
	table.clear(self.connections)
end

return Capture
