--!nonstrict
-- TeleportService in Local Run. A run has one place, so a teleport can only
-- land somewhere the run can reload into: the running place itself, or a
-- local place file the editor maps to the place id. The host hands those to
-- the editor, which restarts the run there with each player's TeleportData,
-- teleport GUI and source place (GetLocalPlayerTeleportData,
-- GetArrivingTeleportGui, Player:GetJoinData). Anywhere else fires
-- TeleportInitFailed with Enum.TeleportResult.Failure, as a refused
-- teleport does. Reserved servers are codes this run made; a teleport to
-- one is a reload like any other.
local host = ...

local M = {}

local teleportGui = nil
local options = {} -- TeleportOptions id -> its TeleportData
local settings = {}
local reserved = {} -- access code -> place id
local serviceId = nil

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

local function int64(value)
	if type(value) ~= "number" then
		fail("Unable to cast " .. typeName(value) .. " to int64")
	end
	return math.floor(value)
end

local function isPlayer(value)
	return typeof(value) == "Instance" and value:IsA("Player")
end

local function serverOnly(method)
	if not host.server() then
		fail(method .. " can only be called from the server")
	end
end

local function localPlayer()
	return game:GetService("Players").LocalPlayer
end

local function encode(data)
	if data == nil then
		return nil
	end
	local ok, json = pcall(host.encode, data)
	if not ok then
		fail("TeleportData could not be serialized: " .. tostring(json))
	end
	return json
end

local function failed(players, placeId, message, teleportOptions)
	task.defer(function()
		for _, player in players do
			host.fire(serviceId, "TeleportInitFailed", player, Enum.TeleportResult.Failure, message, placeId, teleportOptions)
		end
	end)
end

-- Starts the teleport of `players` to `placeId`, or fails it.
local function go(placeId, players, data, code, gui, teleportOptions)
	if placeId ~= game.PlaceId and not host.teleport_ok(placeId) then
		failed(
			players,
			placeId,
			"Local Run can only teleport into this place or a place file mapped to place "
				.. placeId
				.. " in its Local Run settings",
			teleportOptions
		)
		return false
	end
	host.teleport(placeId, players, encode(data), code, gui or teleportGui)
	return true
end

local function playerList(value, method)
	if type(value) ~= "table" then
		fail("Unable to cast " .. typeName(value) .. " to Array")
	end
	if #value == 0 then
		fail(method .. ": players array is empty")
	end
	for _, player in value do
		if not isPlayer(player) then
			fail(method .. ": players array must only contain Player instances")
		end
	end
	return value
end

local SERVICE = {}

function SERVICE.TeleportAsync(self, placeId, players, teleportOptions)
	serviceId = host.id(self)
	serverOnly("TeleportService:TeleportAsync()")
	placeId = int64(placeId)
	players = playerList(players, "TeleportService:TeleportAsync()")
	local data, code = nil, nil
	if teleportOptions ~= nil then
		if typeof(teleportOptions) ~= "Instance" or not teleportOptions:IsA("TeleportOptions") then
			fail("Unable to cast " .. typeName(teleportOptions) .. " to TeleportOptions")
		end
		data = options[host.id(teleportOptions)]
		local given = teleportOptions.ReservedServerId
		if given == "" then
			given = teleportOptions.ReservedServerAccessCode
		end
		if given ~= "" then
			if not reserved[given] then
				fail("TeleportService:TeleportAsync(): invalid reserved server access code")
			end
			code = given
		elseif teleportOptions.ShouldReserveServer then
			code = SERVICE.ReserveServer(self, placeId)
		end
	end
	task.wait()
	go(placeId, players, data, code, nil, teleportOptions)
	local result, resultId = host.new("TeleportAsyncResult", "TeleportAsyncResult")
	if code then
		host.set_raw(resultId, "ReservedServerAccessCode", code)
		host.set_raw(resultId, "PrivateServerId", reserved[code].id)
	end
	return result
end

function SERVICE.Teleport(self, placeId, player, data, gui)
	serviceId = host.id(self)
	placeId = int64(placeId)
	if player == nil and not host.server() then
		player = localPlayer()
	end
	if not isPlayer(player) then
		fail("TeleportService:Teleport(): player must be a Player")
	end
	go(placeId, { player }, data, nil, gui, nil)
end

function SERVICE.TeleportToPlaceInstance(self, placeId, instanceId, player, _spawnName, data, gui)
	serviceId = host.id(self)
	placeId = int64(placeId)
	if type(instanceId) ~= "string" then
		fail("Unable to cast " .. typeName(instanceId) .. " to string")
	end
	if player == nil and not host.server() then
		player = localPlayer()
	end
	if not isPlayer(player) then
		fail("TeleportService:TeleportToPlaceInstance(): player must be a Player")
	end
	go(placeId, { player }, data, nil, gui, nil)
end

function SERVICE.TeleportToSpawnByName(self, placeId, _spawnName, player, data, gui)
	serviceId = host.id(self)
	placeId = int64(placeId)
	if player == nil and not host.server() then
		player = localPlayer()
	end
	if not isPlayer(player) then
		fail("TeleportService:TeleportToSpawnByName(): player must be a Player")
	end
	go(placeId, { player }, data, nil, gui, nil)
end

function SERVICE.TeleportToPrivateServer(self, placeId, code, players, _spawnName, data, gui)
	serviceId = host.id(self)
	serverOnly("TeleportService:TeleportToPrivateServer()")
	placeId = int64(placeId)
	if type(code) ~= "string" then
		fail("Unable to cast " .. typeName(code) .. " to string")
	end
	players = playerList(players, "TeleportService:TeleportToPrivateServer()")
	if not reserved[code] then
		fail("TeleportService:TeleportToPrivateServer(): invalid reserved server access code")
	end
	go(placeId, players, data, code, gui, nil)
end

function SERVICE.TeleportPartyAsync(self, placeId, players, data, gui)
	serviceId = host.id(self)
	serverOnly("TeleportService:TeleportPartyAsync()")
	placeId = int64(placeId)
	players = playerList(players, "TeleportService:TeleportPartyAsync()")
	task.wait()
	go(placeId, players, data, nil, gui, nil)
	return game.JobId
end

function SERVICE.ReserveServer(self, placeId)
	serviceId = host.id(self)
	serverOnly("TeleportService:ReserveServer()")
	placeId = int64(placeId)
	local code = string.gsub(host.guid() .. host.guid(), "-", "")
	local id = host.guid()
	reserved[code] = { place = placeId, id = id }
	return code, id
end

function SERVICE.GetPlayerPlaceInstanceAsync(_, userId)
	userId = int64(userId)
	task.wait()
	if game:GetService("Players"):GetPlayerByUserId(userId) then
		return true, "", game.PlaceId, game.JobId
	end
	return false, "User is not in a game", 0, ""
end

function SERVICE.GetLocalPlayerTeleportData(_)
	local player = localPlayer()
	local arrival = player and host.arrival(player.UserId)
	if not arrival or arrival.data == nil then
		return nil
	end
	return host.decode(arrival.data)
end

function SERVICE.GetArrivingTeleportGui(_)
	local player = localPlayer()
	local arrival = player and host.arrival(player.UserId)
	return arrival and arrival.gui
end

function SERVICE.SetTeleportGui(_, gui)
	if gui ~= nil and (typeof(gui) ~= "Instance" or not gui:IsA("ScreenGui")) then
		fail("Unable to cast " .. typeName(gui) .. " to ScreenGui")
	end
	teleportGui = gui
end

function SERVICE.GetTeleportSetting(_, setting)
	return settings[tostring(setting)]
end

function SERVICE.SetTeleportSetting(_, setting, value)
	settings[tostring(setting)] = value
end

function SERVICE.PromptExperienceDetailsAsync(_)
	task.wait()
	return Enum.PromptExperienceDetailsResult.PromptClosed
end

local OPTIONS = {}

function OPTIONS.SetTeleportData(self, data)
	encode(data)
	options[host.id(self)] = data
end

function OPTIONS.GetTeleportData(self)
	return options[host.id(self)]
end

-- A client that arrived from a teleport hears it once, after the script
-- that first touched the service had the frame to connect.
local announced = false
local function announce()
	if announced or host.server() then
		return
	end
	announced = true
	local player = localPlayer()
	local arrival = player and host.arrival(player.UserId)
	if arrival then
		task.defer(function()
			local data = if arrival.data ~= nil then host.decode(arrival.data) else nil
			host.fire(serviceId, "LocalPlayerArrivedFromTeleport", arrival.gui, data)
		end)
	end
end

local EVENTS = { TeleportInitFailed = true, LocalPlayerArrivedFromTeleport = true }

function M.member(class, id, key)
	if class == "TeleportOptions" then
		return OPTIONS[key], OPTIONS[key] ~= nil
	end
	if class ~= "TeleportService" then
		return nil, false
	end
	serviceId = serviceId or id
	announce()
	if EVENTS[key] then
		return host.signal(id, key), true
	end
	return SERVICE[key], SERVICE[key] ~= nil
end

return M
