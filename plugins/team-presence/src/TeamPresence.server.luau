--!strict
-- Plugin entry point: wires the toolbar button, settings and the publish/poll loops.

local Auth = require(script.Auth)
local Capture = require(script.Capture)
local Markers = require(script.Markers)
local Relay = require(script.Relay)
local Schema = require(script.Schema)
local Secret = require(script.Secret)
local Widget = require(script.Widget)

local Players = game:GetService("Players")

local HEARTBEAT = 5
local TICK = 0.2
local BACKOFF_MIN = 1
local BACKOFF_MAX = 30

local toolbar = plugin:CreateToolbar("rbx-native")
local button = toolbar:CreateButton("Presence", "Show rbx-native team presence", "")
button.ClickableWhenViewportHidden = true

local generation = 0
local capture: any = nil
local markers: any = nil
local relay: any = nil
local widget: any = nil

local function userId(): number
	local id = Players.LocalPlayer and Players.LocalPlayer.UserId or 0
	if id == 0 then
		local ok, studioId = pcall(function()
			return game:GetService("StudioService"):GetUserId()
		end)
		if ok and typeof(studioId) == "number" then
			id = studioId
		end
	end
	return id
end

local function stop()
	generation += 1
	if relay then
		relay:leave()
		relay = nil
	end
	if capture then
		capture:destroy()
		capture = nil
	end
	if markers then
		markers:destroy()
		markers = nil
	end
end

local function nextBackoff(current: number): number
	if current == 0 then
		return BACKOFF_MIN
	end
	return math.min(current * 2, BACKOFF_MAX)
end

local function start()
	stop()
	local relayUrl = plugin:GetSetting("RelayUrl")
	if type(relayUrl) ~= "string" or relayUrl == "" then
		widget:setStatus("Enter a relay URL")
		return
	end
	if game.PlaceId == 0 then
		widget:setStatus("Publish the place first")
		return
	end
	local secret = Secret.read()
	if not secret then
		widget:setNeedsKey(true)
		widget:setStatus("No shared key in this place")
		return
	end
	widget:setNeedsKey(false)

	local key = Auth.deriveKey(secret, game.PlaceId)
	local ownId = userId()
	local myRelay = Relay.new({ url = relayUrl, key = key, room = Auth.roomId(key), userId = ownId })
	local myCapture = Capture.new()
	local myMarkers = Markers.new()
	relay, capture, markers = myRelay, myCapture, myMarkers
	generation += 1
	local myGen = generation
	widget:setStatus("Connecting…")

	task.spawn(function()
		local lastPublish = 0.0
		local backoff = 0
		while generation == myGen do
			if myCapture:isDirty() or os.clock() - lastPublish >= HEARTBEAT then
				myCapture:clearDirty()
				local ok, err = myRelay:publish(myCapture:state())
				if generation ~= myGen then
					return
				end
				if ok then
					lastPublish = os.clock()
					backoff = 0
				else
					widget:setStatus(err or "Publish failed")
					backoff = nextBackoff(backoff)
					lastPublish = os.clock()
					task.wait(backoff)
				end
			end
			task.wait(TICK)
		end
	end)

	task.spawn(function()
		local cursor = 0
		local backoff = 0
		while generation == myGen do
			local snapshot, err = myRelay:poll(cursor)
			if generation ~= myGen then
				return
			end
			if snapshot then
				backoff = 0
				cursor = snapshot.cursor
				local ids = { ownId }
				for _, user in snapshot.users do
					table.insert(ids, user.userId)
				end
				local colors = Schema.assignColors(ids)
				for _, user in snapshot.users do
					user.color = colors[user.userId] or user.color
				end
				local native = {}
				for _, user in snapshot.users do
					if user.userId ~= ownId and user.client == "rbx-native" then
						table.insert(native, user)
					end
				end
				myMarkers:render(snapshot.users, ownId)
				widget:setUsers(native)
				widget:setStatus(`Connected · {#native} rbx-native`)
			else
				widget:setStatus(err or "Poll failed")
				backoff = nextBackoff(backoff)
				task.wait(backoff)
			end
		end
	end)
end

-- Presence on or off is the plugin setting, apart from whether the widget
-- is showing: closing the widget keeps presence running.
local enabled = false

local function apply(on: boolean)
	enabled = on
	plugin:SetSetting("Enabled", on)
	widget:setEnabled(on)
	if on then
		if not plugin:GetSetting("SeenHttpNote") then
			widget:showHttpNote(true)
			plugin:SetSetting("SeenHttpNote", true)
		end
		start()
	else
		stop()
		widget:setStatus("Off")
	end
end

widget = Widget.new(plugin, {
	onToggle = function()
		apply(not enabled)
	end,
	onRelayUrl = function(url: string)
		plugin:SetSetting("RelayUrl", url)
		if enabled then
			start()
		end
	end,
	onCreateKey = function()
		Secret.create()
		if enabled then
			start()
		end
	end,
})

button.Click:Connect(function()
	widget.gui.Enabled = not widget.gui.Enabled
end)

widget.gui:GetPropertyChangedSignal("Enabled"):Connect(function()
	button:SetActive(widget.gui.Enabled)
end)

plugin.Unloading:Connect(stop)

local savedUrl = plugin:GetSetting("RelayUrl")
if type(savedUrl) == "string" then
	widget:setRelayUrl(savedUrl)
end
apply(plugin:GetSetting("Enabled") == true)
