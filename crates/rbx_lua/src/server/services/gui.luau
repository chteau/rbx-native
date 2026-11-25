--!nonstrict
-- GuiService on a Local Run client, past what the engine answers itself
-- (GetGuiInset, IsTenFootInterface): selection (Select, SelectedObject's
-- SelectionLost/SelectionGained), the menu toggles a script can read back,
-- notifications through the core GUI's toast, and stand-ins for the
-- inspect menu, which Local Run doesn't have.
local host = ...

local M = {}

local TOPBAR_INSET = 58

local function fail(message)
	error(message, 0)
end

local function typeName(value)
	if value == nil then
		return "nil"
	end
	if type(value) == "number" then
		return "double"
	end
	return typeof(value)
end

local state = {
	inspect = true,
	emotes = false,
	paused = true,
}
local notifications = 0

local function localPlayer()
	return game:GetService("Players").LocalPlayer
end

local function selectable(object)
	return object:IsA("GuiObject")
		and object.Selectable
		and object.Visible
		and object.AbsoluteSize.X > 0
		and object.AbsoluteSize.Y > 0
end

local SERVICE = {}

function SERVICE.Select(self, parent)
	if typeof(parent) ~= "Instance" then
		fail("Unable to cast value to Object")
	end
	for _, object in parent:GetDescendants() do
		if selectable(object) then
			self.SelectedObject = object
			return
		end
	end
end

function SERVICE.InspectPlayerFromUserId(_, userId)
	if type(userId) ~= "number" then
		fail("Unable to cast " .. typeName(userId) .. " to User")
	end
	local player = game:GetService("Players"):GetPlayerByUserId(userId)
	local who = if player then player.DisplayName else "user " .. userId
	host.notify(localPlayer(), "Inspect", "Local Run has no inspect menu (" .. who .. ").")
end

function SERVICE.InspectPlayerFromHumanoidDescription(_, description, name)
	if typeof(description) ~= "Instance" or not description:IsA("HumanoidDescription") then
		fail("Unable to cast " .. typeName(description) .. " to HumanoidDescription")
	end
	host.notify(localPlayer(), "Inspect", "Local Run has no inspect menu (" .. tostring(name) .. ").")
end

function SERVICE.CloseInspectMenu(_) end

function SERVICE.GetInspectMenuEnabled(_)
	return state.inspect
end

function SERVICE.SetInspectMenuEnabled(_, enabled)
	state.inspect = enabled == true
end

function SERVICE.GetEmotesMenuOpen(_)
	return state.emotes
end

function SERVICE.SetEmotesMenuOpen(_, open)
	state.emotes = open == true
end

function SERVICE.GetGameplayPausedNotificationEnabled(_)
	return state.paused
end

function SERVICE.SetGameplayPausedNotificationEnabled(_, enabled)
	state.paused = enabled == true
end

function SERVICE.SendNotification(_, info)
	if type(info) ~= "table" then
		fail("Unable to cast " .. typeName(info) .. " to Dictionary")
	end
	notifications += 1
	host.notify(localPlayer(), tostring(info.Title or ""), tostring(info.Text or ""))
	return tostring(notifications)
end

function SERVICE.DismissNotification(_, id)
	return type(id) == "string" and tonumber(id) ~= nil and tonumber(id) <= notifications
end

function SERVICE.GetInsetArea(_, insets)
	if typeof(insets) ~= "EnumItem" then
		fail("Unable to cast " .. typeName(insets) .. " to ScreenInsets")
	end
	local size = workspace.CurrentCamera and workspace.CurrentCamera.ViewportSize or Vector2.new(800, 600)
	local top = if insets == Enum.ScreenInsets.None or insets == Enum.ScreenInsets.DeviceSafeInsets then 0 else TOPBAR_INSET
	return Rect.new(0, top, size.X, size.Y)
end

-- TopbarInset: the free strip of the top bar, right of the Roblox logo and
-- the menu and chat buttons our core GUI draws there (12 px margins and
-- gaps, 44 px buttons), 58 px tall like GetGuiInset's top; empty on a
-- server, which has no top bar.
local TOPBAR_LEFT = 12 + 44 + 12 + 44 + 44 + 12
local TOPBAR_RIGHT = 12

local function topbarInset()
	if host.server() then
		return Rect.new(0, 0, 0, 0)
	end
	local size = workspace.CurrentCamera and workspace.CurrentCamera.ViewportSize or Vector2.new(800, 600)
	return Rect.new(TOPBAR_LEFT, 0, math.max(size.X - TOPBAR_RIGHT, TOPBAR_LEFT), TOPBAR_INSET)
end

-- GuiObject:TweenPosition/TweenSize/TweenSizeAndPosition, the legacy
-- tweens: Out and Quad over 1 second by default; a running one is only
-- replaced with `override`, else the call returns false; the callback gets
-- a TweenStatus (Tween.Completed's PlaybackState mapped).
local running = setmetatable({}, { __mode = "k" })

local function legacyTween(object, goal, direction, style, time, override, callback)
	if running[object] then
		if not override then
			return false
		end
		running[object]:Cancel()
	end
	if type(direction) == "string" then
		direction = Enum.EasingDirection[direction]
	end
	if type(style) == "string" then
		style = Enum.EasingStyle[style]
	end
	local info = TweenInfo.new(time or 1, style or Enum.EasingStyle.Quad, direction or Enum.EasingDirection.Out)
	local tween = game:GetService("TweenService"):Create(object, info, goal)
	running[object] = tween
	tween.Completed:Connect(function(status)
		if running[object] == tween then
			running[object] = nil
		end
		if callback then
			local done = status == Enum.PlaybackState.Completed
			task.spawn(callback, if done then Enum.TweenStatus.Completed else Enum.TweenStatus.Canceled)
		end
	end)
	tween:Play()
	return true
end

local GUI_OBJECT = {
	TweenPosition = function(self, position, ...)
		return legacyTween(self, { Position = position }, ...)
	end,
	TweenSize = function(self, size, ...)
		return legacyTween(self, { Size = size }, ...)
	end,
	TweenSizeAndPosition = function(self, size, position, ...)
		return legacyTween(self, { Size = size, Position = position }, ...)
	end,
}

function M.member(class, id, key)
	if class ~= "GuiService" then
		return GUI_OBJECT[key], GUI_OBJECT[key] ~= nil
	end
	if key == "MenuOpened" or key == "MenuClosed" then
		return host.signal(id, key), true
	end
	if key == "TopbarInset" then
		return topbarInset(), true
	end
	return SERVICE[key], SERVICE[key] ~= nil
end

-- SelectedObject is about to change: the old object loses the selection,
-- the new one gains it, then every GuiBase2d holding either (itself
-- included) hears SelectionChanged(amISelected, old, new); the write
-- itself goes on.
function M.set(_, id, _, value)
	local old = game:GetService("GuiService").SelectedObject
	if old == value then
		return false
	end
	local new = if typeof(value) == "Instance" then value else nil
	if old then
		task.defer(host.fire, host.id(old), "SelectionLost")
	end
	if new then
		task.defer(host.fire, host.id(new), "SelectionGained")
	end
	local seen = {}
	for _, object in { old, new } do
		local at = object
		while at do
			if at:IsA("GuiBase2d") and not seen[at] then
				seen[at] = true
				task.defer(host.fire, host.id(at), "SelectionChanged", at == new, old, new)
			end
			at = at.Parent
		end
	end
	return false
end

return M
