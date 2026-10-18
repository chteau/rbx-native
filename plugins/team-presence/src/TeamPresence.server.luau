--!strict
-- Plugin entry point: wires the toolbar button, settings and the publish/poll loops.

local Auth = require(script.Auth)
local Capture = require(script.Capture)
local Catchup = require(script.Catchup)
local Crypto = require(script.Crypto)
local Defaults =require(script.Defaults)
local DiffWindow = require(script.DiffWindow)
local Markers = require(script.Markers)
local Relay = require(script.Relay)
local Schema = require(script.Schema)
local Secret = require(script.Secret)
local Widget = require(script.Widget)

local Players = game:GetService("Players")

local HEARTBEAT = 5
-- How often the last-seen time is saved while connected: what arrives while
-- connected is seen live, so the next catch-up starts after it.
local LAST_SEEN_EVERY = 30
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
local diffWindow: any = nil
-- The journal the last "While you were away" came from, for View changes.
local lastEntries: { any } = {}

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

local function lastSeenKey(): string
	return "LastSeen_" .. tostring(game.PlaceId)
end

local function saveLastSeen()
	if game.PlaceId ~= 0 then
		plugin:SetSetting(lastSeenKey(), DateTime.now().UnixTimestampMillis)
	end
end

local function stop()
	if relay then
		saveLastSeen()
	end
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
	if widget then
		widget:setCatchup(nil)
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
	if game.PlaceId == 0 then
		widget:setStatus("Publish the place first")
		return
	end

	-- Zero-config: the first connector mints the place's secret, everyone
	-- else reads it. Writing it needs a Save/Publish to reach collaborators.
	local secret = Secret.read()
	if not secret then
		secret = Secret.create()
		widget:setStatus("Shared a key — Save or Publish once to share it")
	end

	-- The place names the relay; collaborators read it. A place that names
	-- none falls back to the best-effort community instance.
	local placeRelay = Secret.readRelay()
	local kind = if placeRelay then placeRelay.kind else Defaults.communityKind
	local url = if placeRelay then placeRelay.url else Defaults.communityUrl
	widget:setRelay(kind, url, placeRelay == nil)

	local key = Auth.deriveKey(secret :: string, game.PlaceId)
	local encKey = Crypto.encKey(secret :: string, game.PlaceId)
	local ownId = userId()
	local myRelay = Relay.new({
		kind = kind,
		url = url,
		key = key,
		encKey = encKey,
		room = Auth.roomId(key),
		userId = ownId,
	})
	local myCapture = Capture.new()
	local myMarkers = Markers.new()
	relay, capture, markers = myRelay, myCapture, myMarkers
	generation += 1
	local myGen = generation
	widget:setStatus("Connecting…")

	-- "While you were away": fetch the value-less journal once, scoped to
	-- edits since this client last left this place (kept in plugin settings
	-- per place, mirroring rbx-native's on-disk last-seen).
	task.spawn(function()
		local since = tonumber(plugin:GetSetting(lastSeenKey())) or 0
		local entries = myRelay:journal(since)
		if generation ~= myGen then
			return
		end
		if entries then
			lastEntries = entries
			widget:setCatchup(Catchup.summarize(entries))
		end
		-- From here on, changes arrive live; keep the mark moving.
		while generation == myGen do
			saveLastSeen()
			task.wait(LAST_SEEN_EVERY)
		end
	end)

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
					widget:setStatus(err or "Publish failed", not myRelay:waiting())
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
					if user.client == "rbx-native" then
						if user.userId == ownId then
							user.name = `{user.name} (you, in rbx-native)`
						end
						table.insert(native, user)
					end
				end
				myMarkers:render(snapshot.users)
				widget:setUsers(native)
				widget:setStatus(`Connected · {#native} rbx-native`)
				-- ntfy polls return at once; pace them so we don't spin.
				task.wait(myRelay:pollDelay())
			else
				widget:setStatus(err or "Poll failed", not myRelay:waiting())
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
	onViewChanges = function()
		if not diffWindow then
			diffWindow = DiffWindow.new(plugin)
		end
		diffWindow:show(lastEntries)
	end,
	onSaveRelay = function(kind: string, url: string)
		-- Owner setup: write the relay into the place (empty URL clears it).
		if url == "" then
			Secret.clearRelay()
		else
			Secret.writeRelay(kind, url)
		end
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

local savedRelay = Secret.readRelay()
if savedRelay then
	widget:setRelay(savedRelay.kind, savedRelay.url, false)
else
	widget:setRelay(Defaults.communityKind, Defaults.communityUrl, true)
end
apply(plugin:GetSetting("Enabled") == true)
