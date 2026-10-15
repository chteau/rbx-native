--!strict
--! Tracks which property changes were made by the local user. Studio fires
--! property signals for replicated changes too, so only changes inside a
--! committed ChangeHistory recording (or just after an undo/redo) count.
--! Times are passed in, which keeps the logic testable without Studio.

local Schema = require(script.Parent.Schema)

local Edits = {}

Edits.UNDO_WINDOW_MS = 250
Edits.STALE_RECORDING_MS = 10000
Edits.KEEP_MS = 60000
Edits.MERGE_MS = 2000
local MAX_VALUE = 80

local Tracker = {}
Tracker.__index = Tracker

function Edits.new(): any
	return setmetatable({
		depth = 0,
		startedAt = 0,
		buffer = {},
		undoUntil = -math.huge,
		loose = {},
		noted = {},
	}, Tracker)
end

local function dropStale(self: any, now: number)
	if self.depth > 0 and now - self.startedAt > Edits.STALE_RECORDING_MS then
		self.depth = 0
		self.buffer = {}
	end
end

local function note(self: any, edit: any)
	local value = edit.value
	if type(value) ~= "string" then
		value = tostring(value)
	end
	local text: string = value
	if #text > MAX_VALUE then
		text = text:sub(1, MAX_VALUE)
	end
	for _, existing in ipairs(self.noted) do
		if existing.path == edit.path and existing.property == edit.property and edit.at - existing.at <= Edits.MERGE_MS then
			existing.value = text
			existing.at = edit.at
			return
		end
	end
	table.insert(self.noted, { path = edit.path, property = edit.property, value = text, at = edit.at })
	while #self.noted > Schema.MAX_EDITS do
		table.remove(self.noted, 1)
	end
end

function Tracker.recordingStarted(self: any, now: number)
	dropStale(self, now)
	if self.depth == 0 then
		self.startedAt = now
		self.buffer = {}
	end
	self.depth += 1
end

function Tracker.recordingFinished(self: any, committed: boolean, now: number)
	dropStale(self, now)
	if self.depth == 0 then
		return
	end
	self.depth -= 1
	if self.depth > 0 then
		return
	end
	local buffered = self.buffer
	self.buffer = {}
	if committed then
		for _, edit in ipairs(buffered) do
			note(self, edit)
		end
	end
end

-- Undo and redo apply their changes around the moment OnUndo/OnRedo fires,
-- and the order of the two signals is not documented, so changes from just
-- before it count as well as changes just after.
function Tracker.undoRedo(self: any, now: number)
	self.undoUntil = now + Edits.UNDO_WINDOW_MS
	for _, edit in ipairs(self.loose) do
		if now - edit.at <= Edits.UNDO_WINDOW_MS then
			note(self, edit)
		end
	end
	self.loose = {}
end

function Tracker.changed(self: any, path: string, property: string, value: any, now: number)
	dropStale(self, now)
	local edit = { path = path, property = property, value = value, at = now }
	if self.depth > 0 then
		table.insert(self.buffer, edit)
	elseif now <= self.undoUntil then
		note(self, edit)
	else
		-- Unattributed for now; an undo or redo signalled just after claims it.
		local kept = {}
		for _, old in ipairs(self.loose) do
			if now - old.at <= Edits.UNDO_WINDOW_MS then
				table.insert(kept, old)
			end
		end
		table.insert(kept, edit)
		self.loose = kept
	end
end

function Tracker.recent(self: any, now: number): {any}
	local out: {any} = {}
	for _, edit in ipairs(self.noted) do
		if now - edit.at < Edits.KEEP_MS then
			table.insert(out, { path = edit.path, property = edit.property, value = edit.value, at = edit.at })
		end
	end
	return out
end

return Edits
