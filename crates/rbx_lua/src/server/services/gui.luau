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
	return object:IsA("GuiObject") and object.Selectable and object.Visible
end

local SERVICE = {}

function SERVICE.Select(self, parent)
	if typeof(parent) ~= "Instance" then
		fail("Unable to cast " .. typeName(parent) .. " to Instance")
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
		fail("Unable to cast " .. typeName(userId) .. " to int64")
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

function M.member(_, id, key)
	if key == "MenuOpened" or key == "MenuClosed" then
		return host.signal(id, key), true
	end
	return SERVICE[key], SERVICE[key] ~= nil
end

-- SelectedObject is about to change: the old object loses the selection,
-- the new one gains it; the write itself goes on.
function M.set(_, id, _, value)
	local old = game:GetService("GuiService").SelectedObject
	if old == value then
		return false
	end
	if old then
		task.defer(host.fire, host.id(old), "SelectionLost")
	end
	if typeof(value) == "Instance" then
		task.defer(host.fire, host.id(value), "SelectionGained")
	end
	return false
end

return M
