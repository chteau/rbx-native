--!strict
--! Collects the local user's presence (camera, selection, open script, recent edits) and tracks when it changed.

local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local Selection = game:GetService("Selection")
local StudioService = game:GetService("StudioService")

local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Edits = require(script.Parent.Edits)
local Paths = require(script.Parent.Paths)
local Schema = require(script.Parent.Schema)

local SAMPLE_INTERVAL = 0.2
local MOVE_STUDS = 0.05
local TURN_RADIANS = math.rad(0.5)
local MAX_WATCHED = 50
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
		tracker = Edits.new(),
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
		ChangeHistoryService.OnRecordingStarted:Connect(function()
			self.tracker:recordingStarted(nowMs())
		end)
	)
	table.insert(
		self.connections,
		ChangeHistoryService.OnRecordingFinished:Connect(
			function(_name: string, _displayName: string?, _identifier: string?, operation: Enum.FinishRecordingOperation)
				self.tracker:recordingFinished(operation ~= Enum.FinishRecordingOperation.Cancel, nowMs())
				markDirty()
			end
		)
	)
	local function onUndoRedo()
		self.tracker:undoRedo(nowMs())
		markDirty()
	end
	table.insert(self.connections, ChangeHistoryService.OnUndo:Connect(onUndoRedo))
	table.insert(self.connections, ChangeHistoryService.OnRedo:Connect(onUndoRedo))
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
	self.tracker:changed(path, property, value, nowMs())
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

	local edits = self.tracker:recent(nowMs())

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
