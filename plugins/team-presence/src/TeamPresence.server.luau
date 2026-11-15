--!strict
-- Plugin entry point: wires the toolbar button, settings and the publish/poll loops.

local Auth = require(script.Auth)
local Coedit = require(script.Coedit)
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

local HttpService = game:GetService("HttpService")
local Players = game:GetService("Players")

local HEARTBEAT = 5
-- How often the last-seen time is saved while connected: what arrives while
-- connected is seen live, so the next catch-up starts after it.
local LAST_SEEN_EVERY = 30
local TICK = 1 / 5
local BACKOFF_MIN = 1
local BACKOFF_MAX = 30

local NEW_KEY = Relay.WAITING
	.. " This place had no presence key yet, so I made one. Save the place (File > Save to Roblox)"
	.. " and RbxNative picks it up by itself."

local toolbar = plugin:CreateToolbar("rbx-native")
local button = toolbar:CreateButton("Presence", "Show rbx-native team presence", "")
button.ClickableWhenViewportHidden = true
local coeditButton = toolbar:CreateButton("Co-edit", "Live co-editing (beta): share edits with rbx-native", "")
coeditButton.ClickableWhenViewportHidden = true

local generation = 0
local coedit: any = nil
local coeditEnabled = plugin:GetSetting("coeditEnabled") == true
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
	if coedit then
		coedit:stop()
		coedit = nil
	end
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

-- The status line for a failed publish or poll. Never silent: waiting is
-- said in plain words, with what to do when the two sides can't meet.
-- This install's beacon sender id, kept so reopening the place replaces our
-- own earlier beacon instead of reading as a second Studio on another key.
local function beaconId(): string
	local id = plugin:GetSetting("BeaconFrom")
	if type(id) == "string" and #id == 16 then
		return id
	end
	local fresh = string.sub(string.gsub(HttpService:GenerateGUID(false), "-", ""), 1, 16)
	plugin:SetSetting("BeaconFrom", fresh)
	return fresh
end

local function showProblem(myRelay: any, err: string?, minted: boolean)
	local text = err or "Can't reach the relay"
	if myRelay:waiting() then
		local other = myRelay:keyConflict()
		if other then
			-- RbxNative reads the key saved on Roblox and switches to it by
			-- itself; another Studio can't, so it has to reopen the place.
			widget:setStatus(
				if other == "studio"
					then "Another Studio has this place open with a different presence key, so you can't see each other."
						.. " Save the place from one of them (File > Save to Roblox), then close it and open it again in the other."
					else "RbxNative has this place open with a different presence key."
						.. " Save the place (File > Save to Roblox) and RbxNative switches to this one by itself.",
				true
			)
			return
		end
		if text == Relay.WAITING then
			widget:setStatus(if minted then NEW_KEY else text)
			return
		end
	end
	widget:setStatus(text, string.find(text, "yet (", 1, true) == nil)
end

-- The relay the owner setup names: the kind picked and the URL typed, a
-- blank URL being Auto. A kind that needs a URL but has none is Auto too.
local function ownerChoice(): (string, string)
	local url = string.gsub(widget.urlBox.Text, "^%s*(.-)%s*$", "%1")
	if url == "" then
		return Defaults.communityKind, Defaults.communityUrl
	end
	return widget.relayKind, url
end

local function showRelay(kind: string, url: string)
	widget:setRelay(kind, url, kind == Defaults.communityKind and url == Defaults.communityUrl)
end

local function nextBackoff(current: number): number
	if current == 0 then
		return BACKOFF_MIN
	end
	return math.min(current * 2, BACKOFF_MAX)
end

-- Waits after a failed publish or poll and returns the next backoff. While
-- the room waits for a host the relay only looks every so often anyway, so
-- check back each second to pick a new host up as soon as it is found.
local function retryAfter(myRelay: any, backoff: number): number
	if myRelay:waiting() then
		task.wait(1)
		return 0
	end
	local delay = nextBackoff(backoff)
	task.wait(delay)
	return delay
end

local function start()
	stop()
	if game.PlaceId == 0 then
		widget:setStatus("Publish the place first")
		return
	end

	-- Zero-config: the first editor to turn presence on puts the key and the
	-- relay into the place, everyone else reads them. They reach
	-- collaborators with the next save.
	local secret, minted = Secret.ensure(ownerChoice())

	-- The place names the relay; collaborators read it. A place that names
	-- none falls back to the best-effort community instance.
	local placeRelay = Secret.readRelay()
	local kind = if placeRelay then placeRelay.kind else Defaults.communityKind
	local url = if placeRelay then placeRelay.url else Defaults.communityUrl
	showRelay(kind, url)

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
		placeId = game.PlaceId,
		beaconFrom = beaconId(),
	})
	local myCapture = Capture.new()
	local myMarkers = Markers.new()
	relay, capture, markers = myRelay, myCapture, myMarkers
	generation += 1
	local myGen = generation
	widget:setStatus("Connecting…")

	if coeditEnabled then
		local myCoedit = Coedit.new({
			relay = myRelay,
			userId = ownId,
			room = Auth.roomId(key),
			encKey = encKey,
			name = if Players.LocalPlayer then Players.LocalPlayer.Name else tostring(ownId),
			quiet = { Relay.WAITING, Relay.HOST_GONE },
			onNote = function(note: string)
				warn("[Co-edit] " .. note)
				if generation == myGen then
					widget:setStatus("Co-edit: " .. note)
				end
			end,
		})
		coedit = myCoedit
		myCoedit:start()
	end

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
		local lastPublish = 0
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
					showProblem(myRelay, err, minted)
					lastPublish = os.clock()
					backoff = retryAfter(myRelay, backoff)
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
				showProblem(myRelay, err, minted)
				if myRelay:waiting() then
					-- The host is gone: so are its user and their markers.
					myMarkers:render({})
					widget:setUsers({})
				end
				backoff = retryAfter(myRelay, backoff)
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
		widget:setStatus("Off. Switch on Share my presence to see RbxNative users.")
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
		-- Owner setup saves as soon as it changes. Worker and ntfy wait for
		-- their URL; a blank one is Auto.
		if url == "" and kind ~= Defaults.communityKind then
			widget:setStatus(`Type the {kind} URL. It saves into the place when you press Enter or click away.`)
			return
		end
		local wantKind, wantUrl = ownerChoice()
		local current = Secret.readRelay()
		if current and current.kind == wantKind and current.url == wantUrl then
			return
		end
		Secret.writeRelay(wantKind, wantUrl)
		showRelay(wantKind, wantUrl)
		if enabled then
			start()
		end
	end,
})

button.Click:Connect(function()
	widget.gui.Enabled = not widget.gui.Enabled
end)

coeditButton:SetActive(coeditEnabled)
coeditButton.Click:Connect(function()
	coeditEnabled = not coeditEnabled
	plugin:SetSetting("coeditEnabled", coeditEnabled)
	coeditButton:SetActive(coeditEnabled)
	if coeditEnabled and enabled then
		start()
	elseif coedit then
		coedit:stop()
		coedit = nil
	end
end)

widget.gui:GetPropertyChangedSignal("Enabled"):Connect(function()
	button:SetActive(widget.gui.Enabled)
end)

plugin.Unloading:Connect(stop)

local savedRelay = Secret.readRelay()
if savedRelay then
	showRelay(savedRelay.kind, savedRelay.url)
else
	showRelay(Defaults.communityKind, Defaults.communityUrl)
end
-- First run: show the widget, so the switch that turns presence on is in
-- front of the user instead of behind a toolbar button.
if plugin:GetSetting("Enabled") == nil then
	widget.gui.Enabled = true
end
apply(plugin:GetSetting("Enabled") == true)
